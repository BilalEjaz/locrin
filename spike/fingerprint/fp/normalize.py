import hashlib

from tree_sitter import Node

from fp.parse import parse_snippet

MIN_TOKENS = 40

IDENTIFIER_TYPES = {
    "identifier",
    "property_identifier",
    "shorthand_property_identifier",
    "shorthand_property_identifier_pattern",
    "type_identifier",
    "private_property_identifier",
    "statement_identifier",
}
LITERAL_TYPES = {
    "string",
    "string_fragment",
    "template_string",
    "number",
    "true",
    "false",
    "null",
    "undefined",
    "regex",
}
SKIP_TYPES = {"comment", "'", '"', "`"}


def _walk(node: Node, out: list[str]) -> None:
    if node.type in SKIP_TYPES:
        return
    if node.type in LITERAL_TYPES:
        out.append("LIT")
        return
    if node.type in IDENTIFIER_TYPES:
        out.append("ID")
        return
    if node.child_count == 0:
        out.append(node.type)
        return
    out.append(node.type)
    for child in node.children:
        _walk(child, out)


def tokens(source: str) -> list[str]:
    tree = parse_snippet(source)
    out: list[str] = []
    _walk(tree.root_node, out)
    # drop the program wrapper so equal bodies compare equal regardless of file context
    return [t for t in out if t != "program"]


def structural_hash(toks: list[str]) -> str:
    return hashlib.sha256("\x1f".join(toks).encode("utf8")).hexdigest()
