//! Locator-first structural queries that keep large source bodies out of agent context.

use anyhow::{Result, anyhow, bail};
use regex::RegexBuilder;
use serde::Serialize;

use crate::index::{CodeIndex, SymbolRecord, unique_definition};

/// Half-open UTF-8 byte range and inclusive one-based source-line range.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SourceSpan {
    pub bytes: [usize; 2],
    pub lines: [usize; 2],
}

/// Minimal fresh pointer to a unique function without returning its source.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SymbolLocation {
    pub path: String,
    pub language: String,
    pub name: String,
    pub kind: String,
    pub function: SourceSpan,
    pub signature: SourceSpan,
    pub body: SourceSpan,
    /// Short diagnostic fingerprint; write operations still use the full hash.
    pub snapshot: String,
}

#[derive(Debug, Clone)]
pub struct BodySearchOptions<'a> {
    pub pattern: &'a str,
    pub regex: bool,
    pub ignore_case: bool,
    pub max_results: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BodySearchReport {
    pub location: SymbolLocation,
    pub pattern: String,
    pub mode: String,
    pub ignore_case: bool,
    pub matching_lines: usize,
    pub returned_lines: usize,
    pub truncated: bool,
    pub matches: Vec<BodyLineMatch>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BodyLineMatch {
    pub line: usize,
    pub bytes: [usize; 2],
    pub match_bytes: Vec<[usize; 2]>,
    pub text: String,
}

/// Resolve a unique definition into fresh line/byte spans without returning
/// the potentially large function source.
pub fn locate(index: &CodeIndex, name: &str, file: Option<&str>) -> Result<SymbolLocation> {
    let target = unique_definition(name, index.definitions(name, file)?)?;
    let source = index.fresh_source(&target.path, &target.content_hash, target.parse_ok)?;
    location_from_target(&target, &source)
}

/// Search only the fresh AST body span and return a bounded set of matching
/// source lines with absolute byte ranges.
pub fn search_body(
    index: &CodeIndex,
    name: &str,
    file: Option<&str>,
    options: &BodySearchOptions<'_>,
) -> Result<BodySearchReport> {
    if options.pattern.is_empty() {
        bail!("body search pattern must not be empty");
    }
    if options.max_results == 0 || options.max_results > 1_000 {
        bail!("body search --max-results must be between 1 and 1000");
    }
    let target = unique_definition(name, index.definitions(name, file)?)?;
    let source = index.fresh_source(&target.path, &target.content_hash, target.parse_ok)?;
    let location = location_from_target(&target, &source)?;
    let expression = if options.regex {
        options.pattern.to_owned()
    } else {
        regex::escape(options.pattern)
    };
    let matcher = RegexBuilder::new(&expression)
        .case_insensitive(options.ignore_case)
        .build()
        .map_err(|error| anyhow!("invalid body-search regular expression: {error}"))?;

    let body_start = location.body.bytes[0];
    let body_end = location.body.bytes[1];
    let body = source
        .get(body_start..body_end)
        .ok_or_else(|| anyhow!("indexed body range is outside {}", location.path))?;
    let mut relative_start = 0usize;
    let mut matching_lines = 0usize;
    let mut line_matches = Vec::new();
    for (line, segment) in (location.body.lines[0]..).zip(body.split_inclusive('\n')) {
        let text = segment.trim_end_matches(['\r', '\n']);
        let ranges: Vec<[usize; 2]> = matcher
            .find_iter(text)
            .map(|found| {
                [
                    body_start + relative_start + found.start(),
                    body_start + relative_start + found.end(),
                ]
            })
            .collect();
        if !ranges.is_empty() {
            matching_lines += 1;
            if line_matches.len() < options.max_results {
                line_matches.push(BodyLineMatch {
                    line,
                    bytes: [
                        body_start + relative_start,
                        body_start + relative_start + text.len(),
                    ],
                    match_bytes: ranges,
                    text: text.to_owned(),
                });
            }
        }
        relative_start += segment.len();
    }
    let returned_lines = line_matches.len();
    Ok(BodySearchReport {
        location,
        pattern: options.pattern.to_owned(),
        mode: if options.regex { "regex" } else { "literal" }.to_owned(),
        ignore_case: options.ignore_case,
        matching_lines,
        returned_lines,
        truncated: matching_lines > returned_lines,
        matches: line_matches,
    })
}

fn location_from_target(target: &SymbolRecord, source: &str) -> Result<SymbolLocation> {
    let body_start = target
        .body_start_byte
        .ok_or_else(|| anyhow!("function `{}` has no indexed body", target.name))?;
    let body_end = target
        .body_end_byte
        .ok_or_else(|| anyhow!("function `{}` has no indexed body", target.name))?;
    let snapshot = target
        .content_hash
        .get(..16)
        .unwrap_or(&target.content_hash)
        .to_owned();
    Ok(SymbolLocation {
        path: target.path.clone(),
        language: target.language.clone(),
        name: target.name.clone(),
        kind: target.kind.clone(),
        function: span(source, target.start_byte, target.end_byte)?,
        signature: span(source, target.start_byte, body_start)?,
        body: span(source, body_start, body_end)?,
        snapshot,
    })
}

fn span(source: &str, start: usize, end: usize) -> Result<SourceSpan> {
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        bail!("indexed span {start}..{end} is outside UTF-8 source");
    }
    Ok(SourceSpan {
        bytes: [start, end],
        lines: [line_at(source, start), line_at(source, end - 1)],
    })
}

fn line_at(source: &str, byte: usize) -> usize {
    source[..byte].matches('\n').count() + 1
}
