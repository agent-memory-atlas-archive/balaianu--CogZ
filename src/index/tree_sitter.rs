//! Tree-sitter AST parsing and code entity + edge extraction.
//!
//! Parses source files with tree-sitter in a single pass and extracts
//! code entities (functions, classes, files, modules) with their
//! properties, plus raw structural edges (calls, imports, extends)
//! with name-based references that are resolved to UUIDs by the
//! code_graph module after all files are parsed.
//!
//! Supports Rust, Python, Go, JavaScript, TypeScript, and Bash via
//! per-language extractors.

mod bash;
mod go;
mod javascript;
mod python;
mod raw_edges;
mod rust;
#[cfg(test)]
mod tests;

use std::path::Path;

use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tree_sitter::{Node, Parser};
use tree_sitter_language::LanguageFn;

/// A code entity extracted from source, ready for DB sync.
#[derive(Debug, Clone, Serialize)]
pub struct CodeEntity {
    /// Entity type: "function", "class", "file", "module".
    pub entity_type: &'static str,
    pub title: String,
    pub content: String,
    /// `v2:`-prefixed structural hash of the entity's AST subtree.
    pub semantic_hash: String,
    pub properties: serde_json::Value,
}

impl CodeEntity {
    /// Build an entity from its AST node: content is the node text,
    /// semantic_hash is the structural hash of the node's subtree.
    pub fn new(
        entity_type: &'static str,
        title: String,
        node: &Node,
        source: &[u8],
        properties: serde_json::Value,
    ) -> Option<Self> {
        Some(Self {
            entity_type,
            title,
            content: node_text(node, source)?,
            semantic_hash: ast_hash(node, source),
            properties,
        })
    }
}

/// Structural hash of an AST subtree, insensitive to comments and
/// whitespace by construction (neither reaches the emitted stream).
/// Emits every node's kind in preorder plus the text of named leaves —
/// anonymous leaves (punctuation, operators) are already literals via
/// their kind. Serialized as `v2:{sha256 hex}` to distinguish it from
/// the legacy text-normalized hash.
fn ast_hash(node: &Node, source: &[u8]) -> String {
    let mut hasher = Sha256::new();
    let mut cursor = node.walk();
    let mut descended = false;
    loop {
        if !descended {
            let n = cursor.node();
            if !n.kind().contains("comment") {
                hasher.update(n.kind().as_bytes());
                hasher.update([0]);
                if n.named_child_count() == 0
                    && let Ok(text) = n.utf8_text(source)
                {
                    hasher.update(text.as_bytes());
                    hasher.update([0]);
                }
                if cursor.goto_first_child() {
                    continue;
                }
            }
        }
        if cursor.goto_next_sibling() {
            descended = false;
            continue;
        }
        descended = true;
        if !cursor.goto_parent() {
            break;
        }
    }
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    format!("v2:{hex}")
}

/// A structural edge with unresolved name references.
///
/// Produced during the single-pass AST walk alongside entities.
/// The `code_graph` module resolves `target_name` to a UUID using
/// the global name→UUID map built from all parsed entities.
#[derive(Debug, Clone)]
pub struct RawEdge {
    /// Entity type of the source: "function", "file", "class".
    pub source_type: &'static str,
    /// Qualified name of the source entity (for UUID lookup).
    pub source_name: String,
    /// Unresolved name of the target entity.
    pub target_name: String,
    /// Edge type: "calls", "imports", "extends".
    pub edge_type: &'static str,
}

/// Supported source languages for code indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    Go,
    JavaScript,
    TypeScript,
    Tsx,
    Bash,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Go => "go",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Bash => "bash",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "rust" => Some(Self::Rust),
            "python" => Some(Self::Python),
            "go" => Some(Self::Go),
            "javascript" => Some(Self::JavaScript),
            "typescript" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "bash" => Some(Self::Bash),
            _ => None,
        }
    }

    pub fn tree_sitter_language(&self) -> LanguageFn {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE,
            Self::Python => tree_sitter_python::LANGUAGE,
            Self::Go => tree_sitter_go::LANGUAGE,
            Self::JavaScript => tree_sitter_javascript::LANGUAGE,
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX,
            Self::Bash => tree_sitter_bash::LANGUAGE,
        }
    }
}

/// Parse a source file and extract all code entities.
///
/// Returns a list of `CodeEntity` values: one `file` entity, zero or
/// more `module` entities, and zero or more `function`/`class` entities.
pub fn extract_entities(file_path: &Path, source: &str, language: Language) -> Vec<CodeEntity> {
    let mut parser = Parser::new();
    if parser
        .set_language(&language.tree_sitter_language().into())
        .is_err()
    {
        tracing::warn!(
            "failed to set tree-sitter language for {}",
            file_path.display()
        );
        return Vec::new();
    }

    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None => {
            tracing::warn!("failed to parse {}", file_path.display());
            return Vec::new();
        }
    };

    let path_str = crate::index::path_to_string(file_path);
    let root = tree.root_node();
    let source_bytes = source.as_bytes();

    let mut entities = Vec::new();

    // File entity — always present
    entities.push(file_entity(file_path, &path_str, source, language, &root));

    dispatch_extract(&language, &root, source_bytes, &path_str, &mut entities);

    entities
}

/// Parse a source file once and extract both entities and raw edges.
///
/// This is the single-pass version that avoids re-parsing for edge
/// extraction. Returns the same entities as `extract_entities` plus
/// `RawEdge` values with unresolved name references.
pub fn extract_all(
    file_path: &Path,
    source: &str,
    language: Language,
) -> (Vec<CodeEntity>, Vec<RawEdge>) {
    let mut parser = Parser::new();
    if parser
        .set_language(&language.tree_sitter_language().into())
        .is_err()
    {
        tracing::warn!(
            "failed to set tree-sitter language for {}",
            file_path.display()
        );
        return (Vec::new(), Vec::new());
    }

    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None => {
            tracing::warn!("failed to parse {}", file_path.display());
            return (Vec::new(), Vec::new());
        }
    };

    let path_str = crate::index::path_to_string(file_path);
    let root = tree.root_node();
    let source_bytes = source.as_bytes();

    let mut entities = Vec::new();
    let mut edges = Vec::new();

    // File entity — always present
    entities.push(file_entity(file_path, &path_str, source, language, &root));

    dispatch_extract(&language, &root, source_bytes, &path_str, &mut entities);
    dispatch_raw_edges(&language, &root, source_bytes, &path_str, &mut edges);

    (entities, edges)
}

fn file_entity(
    file_path: &Path,
    path_str: &str,
    source: &str,
    language: Language,
    root: &Node,
) -> CodeEntity {
    CodeEntity {
        entity_type: "file",
        title: file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path_str.to_string()),
        content: source.to_string(),
        semantic_hash: ast_hash(root, source.as_bytes()),
        properties: json!({
            "file_path": path_str,
            "language": language.as_str(),
            "line_count": source.lines().count(),
        }),
    }
}

/// Dispatch entity extraction to the appropriate language module.
fn dispatch_extract(
    language: &Language,
    root: &Node,
    source: &[u8],
    path: &str,
    entities: &mut Vec<CodeEntity>,
) {
    match language {
        Language::Rust => rust::extract_rust(root, source, path, entities),
        Language::Python => python::extract_python(root, source, path, entities),
        Language::Go => go::extract_go(root, source, path, entities),
        Language::JavaScript => {
            javascript::extract_javascript(root, source, path, "javascript", entities)
        }
        Language::TypeScript => {
            javascript::extract_javascript(root, source, path, "typescript", entities)
        }
        Language::Tsx => javascript::extract_javascript(root, source, path, "tsx", entities),
        Language::Bash => bash::extract_bash(root, source, path, entities),
    }
}

/// Dispatch raw edge extraction to the appropriate language module.
fn dispatch_raw_edges(
    language: &Language,
    root: &Node,
    source: &[u8],
    path: &str,
    edges: &mut Vec<RawEdge>,
) {
    match language {
        Language::Rust => raw_edges::extract_rust_raw_edges(root, source, path, edges),
        Language::Python => python::extract_python_raw_edges(root, source, path, edges),
        Language::Go => go::extract_go_raw_edges(root, source, path, edges),
        Language::JavaScript | Language::TypeScript | Language::Tsx => {
            javascript::extract_javascript_raw_edges(root, source, path, edges)
        }
        Language::Bash => bash::extract_bash_raw_edges(root, source, path, edges),
    }
}

pub(super) fn node_text(node: &Node, source: &[u8]) -> Option<String> {
    node.utf8_text(source).ok().map(|s| s.to_string())
}

pub(super) fn walk_descendants<F>(node: &Node, f: &mut F)
where
    F: FnMut(&Node) -> bool,
{
    let num_named = node.named_child_count();
    for i in 0..num_named {
        // tree-sitter 0.26 indexes children with u32; child counts
        // can't exceed u32::MAX in the underlying C API, so the
        // cast can't truncate.
        if let Some(child) = node.named_child(i as u32) {
            if !f(&child) {
                return;
            }
            walk_descendants(&child, f);
        }
    }
}
