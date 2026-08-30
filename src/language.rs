//! Tree-sitter language adapters and the common function/call schema.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, Tree};

/// Statically typed languages supported by the portable structural core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    C,
    Cpp,
    Go,
    #[serde(rename = "objective-c")]
    ObjectiveC,
    Glsl,
    Kotlin,
}

impl Language {
    #[must_use]
    pub fn for_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "rs" => Some(Self::Rust),
            "c" | "h" => Some(Self::C),
            "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Some(Self::Cpp),
            "go" => Some(Self::Go),
            "m" => Some(Self::ObjectiveC),
            "vert" | "frag" | "glsl" | "geom" | "comp" | "tesc" | "tese" => Some(Self::Glsl),
            "kt" | "kts" => Some(Self::Kotlin),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Go => "go",
            Self::ObjectiveC => "objective-c",
            Self::Glsl => "glsl",
            Self::Kotlin => "kotlin",
        }
    }

    fn grammar(self) -> tree_sitter::Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::ObjectiveC => tree_sitter_objc::LANGUAGE.into(),
            Self::Glsl => tree_sitter_glsl::LANGUAGE_GLSL.into(),
            Self::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
        }
    }
}

/// One indexed function definition or C prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub start_byte: usize,
    pub end_byte: usize,
    pub name_start_byte: usize,
    pub name_end_byte: usize,
    pub body_start_byte: Option<usize>,
    pub body_end_byte: Option<usize>,
    pub parameters_start_byte: Option<usize>,
    pub parameters_end_byte: Option<usize>,
    pub start_line: usize,
    pub end_line: usize,
}

/// Function records distinguish an implementation from a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Definition,
    Prototype,
}

impl SymbolKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Prototype => "prototype",
        }
    }
}

/// One AST-confirmed function or method call name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallReference {
    pub name: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    pub arguments_start_byte: Option<usize>,
    pub arguments_end_byte: Option<usize>,
}

/// Parsed structural facts for one source file.
#[derive(Debug)]
pub struct ParsedSource {
    pub tree: Tree,
    pub functions: Vec<FunctionSymbol>,
    pub calls: Vec<CallReference>,
}

pub fn parse(language: Language, source: &str) -> Result<ParsedSource> {
    let mut parser = Parser::new();
    parser
        .set_language(&language.grammar())
        .with_context(|| format!("load {} grammar", language.as_str()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("tree-sitter returned no parse tree"))?;
    let mut functions = Vec::new();
    let mut calls = Vec::new();
    walk(
        tree.root_node(),
        source.as_bytes(),
        language,
        &mut functions,
        &mut calls,
    );
    Ok(ParsedSource {
        tree,
        functions,
        calls,
    })
}

fn walk(
    node: Node<'_>,
    source: &[u8],
    language: Language,
    functions: &mut Vec<FunctionSymbol>,
    calls: &mut Vec<CallReference>,
) {
    if let Some(symbol) = function_symbol(node, source, language) {
        functions.push(symbol);
    }
    if let Some(reference) = call_reference(node, source, language) {
        calls.push(reference);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(child, source, language, functions, calls);
    }
}

fn function_symbol(node: Node<'_>, source: &[u8], language: Language) -> Option<FunctionSymbol> {
    match language {
        Language::Rust if node.kind() == "function_item" => {
            let name = node.child_by_field_name("name")?;
            let body = node.child_by_field_name("body")?;
            let parameters = node.child_by_field_name("parameters");
            Some(make_symbol(
                node,
                name,
                Some(body),
                parameters,
                SymbolKind::Definition,
                source,
            ))
        }
        Language::C | Language::Cpp | Language::ObjectiveC | Language::Glsl
            if node.kind() == "function_definition" =>
        {
            let declarator = node.child_by_field_name("declarator")?;
            let name = c_declarator_name(declarator)?;
            let body = node.child_by_field_name("body")?;
            let parameters = c_function_declarator(declarator)
                .and_then(|value| value.child_by_field_name("parameters"));
            Some(make_symbol(
                node,
                name,
                Some(body),
                parameters,
                SymbolKind::Definition,
                source,
            ))
        }
        Language::C | Language::Cpp | Language::ObjectiveC | Language::Glsl
            if node.kind() == "declaration" =>
        {
            let declarator = c_function_declarator(node)?;
            if declarator
                .child_by_field_name("declarator")
                .is_some_and(|child| child.kind() == "parenthesized_declarator")
            {
                return None;
            }
            let name = c_declarator_name(declarator)?;
            let parameters = declarator.child_by_field_name("parameters");
            Some(make_symbol(
                node,
                name,
                None,
                parameters,
                SymbolKind::Prototype,
                source,
            ))
        }
        Language::Go if matches!(node.kind(), "function_declaration" | "method_declaration") => {
            let body = node.child_by_field_name("body")?;
            Some(make_symbol(
                node,
                node.child_by_field_name("name")?,
                Some(body),
                node.child_by_field_name("parameters"),
                SymbolKind::Definition,
                source,
            ))
        }
        Language::Kotlin if node.kind() == "function_declaration" => {
            kotlin_function_symbol(node, source)
        }
        Language::ObjectiveC if node.kind() == "method_definition" => {
            let name = objc_method_name(node)?;
            let body = named_descendant(node, "compound_statement")?;
            Some(make_symbol(
                node,
                name,
                Some(body),
                None,
                SymbolKind::Definition,
                source,
            ))
        }
        Language::ObjectiveC if node.kind() == "method_declaration" => {
            let name = objc_method_name(node)?;
            Some(make_symbol(
                node,
                name,
                None,
                None,
                SymbolKind::Prototype,
                source,
            ))
        }
        _ => None,
    }
}

fn kotlin_function_symbol(node: Node<'_>, source: &[u8]) -> Option<FunctionSymbol> {
    let body = direct_named_child(node, "function_body");
    Some(make_symbol(
        node,
        node.child_by_field_name("name")?,
        body,
        direct_named_child(node, "function_value_parameters"),
        if body.is_some() {
            SymbolKind::Definition
        } else {
            SymbolKind::Prototype
        },
        source,
    ))
}

fn make_symbol(
    node: Node<'_>,
    name_node: Node<'_>,
    body: Option<Node<'_>>,
    parameters: Option<Node<'_>>,
    kind: SymbolKind,
    source: &[u8],
) -> FunctionSymbol {
    FunctionSymbol {
        name: node_text(name_node, source).to_owned(),
        kind,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        name_start_byte: name_node.start_byte(),
        name_end_byte: name_node.end_byte(),
        body_start_byte: body.map(|value| value.start_byte()),
        body_end_byte: body.map(|value| value.end_byte()),
        parameters_start_byte: parameters.map(|value| value.start_byte()),
        parameters_end_byte: parameters.map(|value| value.end_byte()),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
    }
}

fn c_function_declarator(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "function_declarator" {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find_map(|child| {
        if child.kind() == "function_declarator" {
            Some(child)
        } else if matches!(
            child.kind(),
            "pointer_declarator" | "init_declarator" | "parenthesized_declarator"
        ) {
            c_function_declarator(child)
        } else {
            None
        }
    })
}

fn c_declarator_name(node: Node<'_>) -> Option<Node<'_>> {
    if matches!(node.kind(), "identifier" | "field_identifier") {
        return Some(node);
    }
    if let Some(child) = node.child_by_field_name("declarator")
        && let Some(name) = c_declarator_name(child)
    {
        return Some(name);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find_map(c_declarator_name)
}

fn named_descendant<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| named_descendant(child, kind))
}

fn direct_named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn objc_method_name(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "identifier" {
            return Some(child);
        }
        if matches!(child.kind(), "keyword_declarator" | "method_parameter")
            && let Some(name) = named_descendant(child, "identifier")
        {
            return Some(name);
        }
    }
    None
}

fn call_reference(node: Node<'_>, source: &[u8], language: Language) -> Option<CallReference> {
    let (name_node, arguments) = match language {
        Language::Rust
        | Language::C
        | Language::Cpp
        | Language::Go
        | Language::ObjectiveC
        | Language::Glsl
            if node.kind() == "call_expression" =>
        {
            let name = terminal_callable_name(node.child_by_field_name("function")?)?;
            (
                name,
                node.child_by_field_name("arguments")
                    .map(|value| (value.start_byte(), value.end_byte())),
            )
        }
        Language::Rust if node.kind() == "method_call_expression" => (
            node.child_by_field_name("name")?,
            node.child_by_field_name("arguments")
                .map(|value| (value.start_byte(), value.end_byte())),
        ),
        Language::ObjectiveC if node.kind() == "message_expression" => {
            (node.child_by_field_name("method")?, None)
        }
        Language::Kotlin if node.kind() == "call_expression" => {
            let callee = node.named_child(0)?;
            // Kotlin represents `target(1) { ... }` as an outer call whose
            // callee is the complete inner `target(1)` call. Index only the
            // inner call so rename/argument edits are not duplicated.
            if callee.kind() == "call_expression" {
                return None;
            }
            let name = terminal_callable_name(callee)?;
            let has_trailing_lambda = direct_named_child(node, "annotated_lambda").is_some()
                || node.parent().is_some_and(|parent| {
                    parent.kind() == "call_expression"
                        && direct_named_child(parent, "annotated_lambda").is_some()
                });
            // Appending a parameter can change which formal receives a trailing
            // lambda, so retain the call for rename/refs but deliberately omit
            // an editable argument span. append-parameter will refuse it.
            let arguments = (!has_trailing_lambda)
                .then(|| direct_named_child(node, "value_arguments"))
                .flatten()
                .map(|value| (value.start_byte(), value.end_byte()));
            (name, arguments)
        }
        _ => return None,
    };
    Some(CallReference {
        name: node_text(name_node, source).to_owned(),
        start_byte: name_node.start_byte(),
        end_byte: name_node.end_byte(),
        line: name_node.start_position().row + 1,
        arguments_start_byte: arguments.map(|value| value.0),
        arguments_end_byte: arguments.map(|value| value.1),
    })
}

fn terminal_callable_name(node: Node<'_>) -> Option<Node<'_>> {
    if matches!(node.kind(), "identifier" | "field_identifier") {
        return Some(node);
    }
    for field in ["name", "field", "function"] {
        if let Some(child) = node.child_by_field_name(field)
            && let Some(name) = terminal_callable_name(child)
        {
            return Some(name);
        }
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter_map(terminal_callable_name)
        .last()
}

fn node_text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.byte_range()]).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_extracts_defs_methods_and_calls_without_comment_noise() {
        let source = r#"
fn alpha() { beta(); }
impl Widget { fn beta(&self) { self.gamma(); } }
// beta() and gamma() are not calls here.
const TEXT: &str = "beta()";
"#;
        let parsed = parse(Language::Rust, source).unwrap();
        let defs: Vec<_> = parsed
            .functions
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        let calls: Vec<_> = parsed.calls.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(defs, ["alpha", "beta"]);
        assert_eq!(calls, ["beta", "gamma"]);
    }

    #[test]
    fn c_extracts_definition_prototype_and_calls_but_not_function_pointer_variable() {
        let source = r"
int helper(int value);
int (*callback)(int);
static int helper(int value) { return value; }
int run(void) { return helper(4); }
";
        let parsed = parse(Language::C, source).unwrap();
        let facts: Vec<_> = parsed
            .functions
            .iter()
            .map(|item| (item.name.as_str(), item.kind))
            .collect();
        assert_eq!(
            facts,
            [
                ("helper", SymbolKind::Prototype),
                ("helper", SymbolKind::Definition),
                ("run", SymbolKind::Definition),
            ]
        );
        assert_eq!(parsed.calls.len(), 1);
        assert_eq!(parsed.calls[0].name, "helper");
        assert!(parsed.functions[0].parameters_start_byte.is_some());
        assert!(parsed.calls[0].arguments_start_byte.is_some());
    }

    #[test]
    fn cpp_extracts_free_and_inline_member_functions() {
        let source = r"
int helper(int value);
class Worker {
public:
    int run(int value) { return helper(value); }
};
int helper(int value) { return value + 1; }
";
        let parsed = parse(Language::Cpp, source).unwrap();
        let facts: Vec<_> = parsed
            .functions
            .iter()
            .map(|item| (item.name.as_str(), item.kind))
            .collect();
        assert_eq!(
            facts,
            [
                ("helper", SymbolKind::Prototype),
                ("run", SymbolKind::Definition),
                ("helper", SymbolKind::Definition),
            ]
        );
        assert_eq!(parsed.calls[0].name, "helper");
    }

    #[test]
    fn go_extracts_generic_functions_methods_and_selector_calls() {
        let source = r"
package sample

func helper[T ~int](value T) T { return value }
func (worker *Worker) Run(value int) int {
    return helper(value) + worker.Next(value)
}
";
        let parsed = parse(Language::Go, source).unwrap();
        let defs: Vec<_> = parsed
            .functions
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        let calls: Vec<_> = parsed.calls.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(defs, ["helper", "Run"]);
        assert_eq!(calls, ["helper", "Next"]);
        assert!(
            parsed
                .functions
                .iter()
                .all(|item| item.parameters_start_byte.is_some())
        );
        assert!(
            parsed
                .calls
                .iter()
                .all(|item| item.arguments_start_byte.is_some())
        );
    }

    #[test]
    fn objective_c_extracts_method_declarations_definitions_and_messages() {
        let source = r"
@interface Worker
- (void)start;
- (int)consume:(int)value;
@end
@implementation Worker
- (void)start { [self consume:4]; }
- (int)consume:(int)value { return value; }
@end
";
        let parsed = parse(Language::ObjectiveC, source).unwrap();
        let facts: Vec<_> = parsed
            .functions
            .iter()
            .map(|item| (item.name.as_str(), item.kind))
            .collect();
        assert_eq!(
            facts,
            [
                ("start", SymbolKind::Prototype),
                ("consume", SymbolKind::Prototype),
                ("start", SymbolKind::Definition),
                ("consume", SymbolKind::Definition),
            ]
        );
        assert_eq!(parsed.calls[0].name, "consume");
    }

    #[test]
    fn glsl_extracts_prototypes_definitions_and_builtin_calls() {
        let source = r"
float shade(float value);
float shade(float value) { return sin(value); }
void main() { float result = shade(1.0); }
";
        let parsed = parse(Language::Glsl, source).unwrap();
        let facts: Vec<_> = parsed
            .functions
            .iter()
            .map(|item| (item.name.as_str(), item.kind))
            .collect();
        assert_eq!(
            facts,
            [
                ("shade", SymbolKind::Prototype),
                ("shade", SymbolKind::Definition),
                ("main", SymbolKind::Definition),
            ]
        );
        let calls: Vec<_> = parsed.calls.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(calls, ["sin", "shade"]);
    }

    #[test]
    fn kotlin_extracts_expression_braced_generic_extension_and_trailing_lambda_calls() {
        let source = r#"
package sample

fun easy(value: Int): Int = value + 1
fun String.decorate(prefix: String): String { return prefix + this }

class Worker {
    suspend fun <T> complex(value: T, transform: (T) -> T): T
        where T : Any {
        val first = transform(value)
        return helper(first) + this.finish(first)
    }
}

fun helper(value: Any): Any { return value }
fun caller(worker: Worker) {
    worker.complex(1) { it }
    "value".decorate("prefix")
}
"#;
        let parsed = parse(Language::Kotlin, source).unwrap();
        assert!(!parsed.tree.root_node().has_error());
        let facts: Vec<_> = parsed
            .functions
            .iter()
            .map(|item| (item.name.as_str(), item.kind))
            .collect();
        assert_eq!(
            facts,
            [
                ("easy", SymbolKind::Definition),
                ("decorate", SymbolKind::Definition),
                ("complex", SymbolKind::Definition),
                ("helper", SymbolKind::Definition),
                ("caller", SymbolKind::Definition),
            ]
        );
        let calls: Vec<_> = parsed.calls.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(
            calls,
            ["transform", "helper", "finish", "complex", "decorate"]
        );
        assert!(
            parsed
                .functions
                .iter()
                .all(|item| item.parameters_start_byte.is_some())
        );
        let complex = parsed
            .calls
            .iter()
            .find(|item| item.name == "complex")
            .unwrap();
        assert!(complex.arguments_start_byte.is_none());
        let decorate = parsed
            .calls
            .iter()
            .find(|item| item.name == "decorate")
            .unwrap();
        assert!(decorate.arguments_start_byte.is_some());
    }
}
