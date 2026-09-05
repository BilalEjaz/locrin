from dataclasses import dataclass
from pathlib import Path

from tree_sitter import Node, Tree

from fp.parse import parse_source

FUNCTION_TYPES = {
    "function_declaration",
    "generator_function_declaration",
    "method_definition",
    "arrow_function",
    "function_expression",
    "function",  # older grammar name for function_expression
}


@dataclass(frozen=True)
class FunctionRecord:
    id: str
    file: str
    name: str
    start_line: int
    end_line: int
    source: str


def function_nodes(tree: Tree) -> list[Node]:
    found: list[Node] = []
    stack = [tree.root_node]
    while stack:
        node = stack.pop()
        if node.type in FUNCTION_TYPES:
            found.append(node)
        stack.extend(reversed(node.children))
    found.sort(key=lambda n: n.start_byte)
    return found


def _name_of(node: Node) -> str:
    name_node = node.child_by_field_name("name")
    if name_node is not None:
        return name_node.text.decode("utf8")
    parent = node.parent
    if parent is not None and parent.type == "variable_declarator":
        n = parent.child_by_field_name("name")
        if n is not None:
            return n.text.decode("utf8")
    if parent is not None and parent.type == "pair":
        k = parent.child_by_field_name("key")
        if k is not None:
            return k.text.decode("utf8")
    return "<anon>"


def extract_functions(path: str, source: str) -> list[FunctionRecord]:
    file = str(Path(path).resolve()).replace("\\", "/")
    tree = parse_source(source, tsx=file.endswith(".tsx"))
    src_bytes = source.encode("utf8")
    out: list[FunctionRecord] = []
    for node in function_nodes(tree):
        start_line = node.start_point[0] + 1
        end_line = node.end_point[0] + 1
        name = _name_of(node)
        text = src_bytes[node.start_byte:node.end_byte].decode("utf8", errors="replace")
        out.append(FunctionRecord(
            id=f"{file}:{start_line}:{name}",
            file=file,
            name=name,
            start_line=start_line,
            end_line=end_line,
            source=text,
        ))
    return out
