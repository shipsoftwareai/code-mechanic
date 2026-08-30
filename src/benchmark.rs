//! Reproducible retrieval output/token and latency benchmark.

use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tiktoken_rs::o200k_base_singleton;

use crate::index::{CodeIndex, SymbolSource};
use crate::query;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkCase {
    pub symbol: String,
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BenchmarkReport {
    pub schema_version: u32,
    pub tokenizer: String,
    pub baseline: String,
    pub indexed: String,
    pub locator: String,
    pub warm_runs: usize,
    pub minimum_token_reduction_pct: f64,
    pub passed: bool,
    pub aggregate: AggregateReport,
    pub cases: Vec<CaseReport>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AggregateReport {
    pub cases_passed: usize,
    pub cases_total: usize,
    pub baseline_bytes: usize,
    pub indexed_bytes: usize,
    pub locator_bytes: usize,
    pub baseline_tokens: usize,
    pub indexed_tokens: usize,
    pub locator_tokens: usize,
    pub token_reduction_pct: f64,
    pub locator_vs_full_source_reduction_pct: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CaseReport {
    pub symbol: String,
    pub file: String,
    pub answer_equivalent: bool,
    pub locator_exact: bool,
    pub baseline_bytes: usize,
    pub indexed_bytes: usize,
    pub locator_bytes: usize,
    pub baseline_tokens: usize,
    pub indexed_tokens: usize,
    pub locator_tokens: usize,
    pub token_reduction_pct: f64,
    pub locator_vs_full_source_reduction_pct: f64,
    pub baseline_scan_p50_us: u128,
    pub indexed_query_p50_us: u128,
    pub locator_query_p50_us: u128,
    pub passed: bool,
}

/// Compare a realistic text-search + targeted range read with the exact
/// indexed function response. Both arms use the same o200k tokenizer and must
/// contain the exact function source before token savings count as a pass.
pub fn run(
    index: &CodeIndex,
    cases: &[BenchmarkCase],
    warm_runs: usize,
    window_lines: usize,
    minimum_reduction_pct: f64,
) -> Result<BenchmarkReport> {
    if cases.is_empty() {
        bail!("benchmark requires at least one --case SYMBOL[:FILE]");
    }
    if warm_runs == 0 {
        bail!("benchmark warm run count must be positive");
    }
    if window_lines < 20 {
        bail!("benchmark targeted range must be at least 20 lines");
    }
    let mut reports = Vec::with_capacity(cases.len());
    for case in cases {
        reports.push(run_case(
            index,
            case,
            warm_runs,
            window_lines,
            minimum_reduction_pct,
        )?);
    }
    let passed = reports.iter().all(|report| report.passed);
    let baseline_bytes = reports.iter().map(|report| report.baseline_bytes).sum();
    let indexed_bytes = reports.iter().map(|report| report.indexed_bytes).sum();
    let locator_bytes = reports.iter().map(|report| report.locator_bytes).sum();
    let baseline_tokens = reports.iter().map(|report| report.baseline_tokens).sum();
    let indexed_tokens = reports.iter().map(|report| report.indexed_tokens).sum();
    let locator_tokens = reports.iter().map(|report| report.locator_tokens).sum();
    let aggregate = AggregateReport {
        cases_passed: reports.iter().filter(|report| report.passed).count(),
        cases_total: reports.len(),
        baseline_bytes,
        indexed_bytes,
        locator_bytes,
        baseline_tokens,
        indexed_tokens,
        locator_tokens,
        token_reduction_pct: reduction_pct(baseline_tokens, indexed_tokens),
        locator_vs_full_source_reduction_pct: reduction_pct(indexed_tokens, locator_tokens),
    };
    Ok(BenchmarkReport {
        schema_version: 2,
        tokenizer: "o200k_base (local tiktoken-rs)".to_owned(),
        baseline: format!(
            "in-process exact-word workspace scan plus one {window_lines}-line targeted source range"
        ),
        indexed: "exact raw indexed function source (the symbol --raw payload)".to_owned(),
        locator: "fresh function/signature/body spans without source (compact locate JSON)"
            .to_owned(),
        warm_runs,
        minimum_token_reduction_pct: minimum_reduction_pct,
        passed,
        aggregate,
        cases: reports,
    })
}

fn run_case(
    index: &CodeIndex,
    case: &BenchmarkCase,
    warm_runs: usize,
    window_lines: usize,
    minimum_reduction_pct: f64,
) -> Result<CaseReport> {
    let indexed = index.read_symbol(&case.symbol, case.file.as_deref())?;
    let mut baseline_times = Vec::with_capacity(warm_runs);
    let mut indexed_times = Vec::with_capacity(warm_runs);
    let mut locator_times = Vec::with_capacity(warm_runs);
    let mut baseline_output = String::new();
    let mut indexed_output = String::new();
    let mut locator_output = String::new();
    let mut indexed_answer_equivalent = false;
    let live_source = std::fs::read_to_string(index.root().join(&indexed.path))
        .with_context(|| format!("read benchmark target {}", indexed.path))?;
    let mut locator_exact = false;
    for _ in 0..warm_runs {
        let baseline_start = Instant::now();
        baseline_output = baseline_output_for(index, &indexed, window_lines)?;
        baseline_times.push(baseline_start.elapsed().as_micros());

        let indexed_start = Instant::now();
        let response = index.read_symbol(&case.symbol, case.file.as_deref())?;
        indexed_answer_equivalent = response.source == indexed.source;
        indexed_output.clone_from(&response.source);
        indexed_times.push(indexed_start.elapsed().as_micros());

        let locator_start = Instant::now();
        let location = query::locate(index, &case.symbol, case.file.as_deref())?;
        locator_exact = live_source.get(location.function.bytes[0]..location.function.bytes[1])
            == Some(indexed.source.as_str());
        locator_output = serde_json::to_string(&location)?;
        locator_times.push(locator_start.elapsed().as_micros());
    }
    let answer_equivalent = baseline_output.contains(&indexed.source) && indexed_answer_equivalent;
    let tokenizer = o200k_base_singleton();
    let baseline_tokens = tokenizer.encode_with_special_tokens(&baseline_output).len();
    let indexed_tokens = tokenizer.encode_with_special_tokens(&indexed_output).len();
    let locator_tokens = tokenizer.encode_with_special_tokens(&locator_output).len();
    let reduction = reduction_pct(baseline_tokens, indexed_tokens);
    Ok(CaseReport {
        symbol: case.symbol.clone(),
        file: indexed.path,
        answer_equivalent,
        locator_exact,
        baseline_bytes: baseline_output.len(),
        indexed_bytes: indexed_output.len(),
        locator_bytes: locator_output.len(),
        baseline_tokens,
        indexed_tokens,
        locator_tokens,
        token_reduction_pct: reduction,
        locator_vs_full_source_reduction_pct: reduction_pct(indexed_tokens, locator_tokens),
        baseline_scan_p50_us: median(&mut baseline_times),
        indexed_query_p50_us: median(&mut indexed_times),
        locator_query_p50_us: median(&mut locator_times),
        passed: answer_equivalent && locator_exact && reduction >= minimum_reduction_pct,
    })
}

fn baseline_output_for(
    index: &CodeIndex,
    target: &SymbolSource,
    window_lines: usize,
) -> Result<String> {
    let mut output = String::new();
    for relative in index.indexed_files()? {
        let absolute = index.root().join(&relative);
        let Ok(source) = std::fs::read_to_string(&absolute) else {
            continue;
        };
        for (line_index, line) in source.lines().enumerate() {
            if contains_word(line, &target.name) {
                writeln!(output, "{relative}:{}:{line}", line_index + 1)?;
            }
        }
    }

    let target_source = std::fs::read_to_string(index.root().join(&target.path))
        .with_context(|| format!("read benchmark target {}", target.path))?;
    // Keep original line terminators so the range contains the byte-exact
    // indexed answer on CRLF checkouts as well as LF checkouts.
    let lines: Vec<&str> = target_source.split_inclusive('\n').collect();
    let function_lines = target.end_line.saturating_sub(target.start_line) + 1;
    let effective_window = window_lines.max(function_lines + 10);
    let leading = effective_window.saturating_sub(function_lines) / 2;
    let start = target.start_line.saturating_sub(leading + 1);
    let end = (start + effective_window).min(lines.len());
    writeln!(output, "{}:{}-{}", target.path, start + 1, end)?;
    for line in &lines[start..end] {
        output.push_str(line);
        if !line.ends_with('\n') {
            output.push('\n');
        }
    }
    Ok(output)
}

fn contains_word(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(index, _)| {
        let before = line[..index].chars().next_back();
        let after = line[index + needle.len()..].chars().next();
        !before.is_some_and(is_identifier_continue) && !after.is_some_and(is_identifier_continue)
    })
}

const fn is_identifier_continue(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn reduction_pct(baseline: usize, indexed: usize) -> f64 {
    if baseline == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let ratio = indexed as f64 / baseline as f64;
    100.0 * (1.0 - ratio)
}

fn median(values: &mut [u128]) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

/// Parse `name` or `name:path` without treating Windows drive separators as a
/// file filter when the left side is empty.
pub fn parse_case(value: &str) -> Result<BenchmarkCase> {
    let (symbol, file) = value
        .split_once(':')
        .map_or((value, None), |(symbol, file)| {
            (symbol, Some(file.to_owned()))
        });
    if symbol.is_empty() {
        bail!("benchmark case has an empty symbol: {value}");
    }
    if file.as_deref() == Some("") {
        bail!("benchmark case has an empty file filter: {value}");
    }
    if file
        .as_deref()
        .is_some_and(|path| Path::new(path).is_absolute())
    {
        bail!("benchmark file filter must be workspace-relative: {value}");
    }
    Ok(BenchmarkCase {
        symbol: symbol.to_owned(),
        file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_word_filter_avoids_identifier_substrings() {
        assert!(contains_word("call target();", "target"));
        assert!(!contains_word("call target_extra();", "target"));
        assert!(!contains_word("call my_target();", "target"));
    }

    #[test]
    fn case_parser_supports_optional_file() {
        assert_eq!(
            parse_case("alpha:src/lib.rs").unwrap(),
            BenchmarkCase {
                symbol: "alpha".to_owned(),
                file: Some("src/lib.rs".to_owned()),
            }
        );
    }
}
