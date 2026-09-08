//! Small helpers over tree-sitter nodes shared by the extractors and the rules.

use tree_sitter::Node;

/// The source text of a node, or `""` when the span is not valid UTF-8.
pub fn text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

/// The 1-based line a node starts on.
pub fn line(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

/// Whether `node` has an anonymous child token spelled `keyword`, such as the
/// `default` in `export default` or the `type` in `import type`. Keyword tokens
/// are anonymous nodes whose kind is the keyword itself, so this is the only
/// way to see them; `to_sexp` never prints them.
pub fn has_keyword(node: Node, keyword: &str) -> bool {
    // Two locals, not one expression: the iterator borrows `cursor`, so it has to
    // be dropped before `cursor` is, which is what reverse declaration order gives.
    let mut cursor = node.walk();
    let mut children = node.children(&mut cursor);
    children.any(|c| !c.is_named() && c.kind() == keyword)
}
