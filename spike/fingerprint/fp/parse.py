from pathlib import Path

import tree_sitter_typescript as tsts
from tree_sitter import Language, Parser, Tree

TS_LANGUAGE = Language(tsts.language_typescript())
TSX_LANGUAGE = Language(tsts.language_tsx())


def parse_source(source: str, tsx: bool = False) -> Tree:
    parser = Parser(TSX_LANGUAGE if tsx else TS_LANGUAGE)
    return parser.parse(source.encode("utf8"))


def parse_file(path: str) -> Tree:
    p = Path(path)
    return parse_source(p.read_text(encoding="utf8", errors="replace"), tsx=p.suffix == ".tsx")


def parse_snippet(source: str) -> Tree:
    """Parse the source of a single function.

    extract.FunctionRecord.source is the exact node text, so a method arrives as a bare
    method body ("async load(id) { ... }"). That is not valid standalone TypeScript: the
    grammar only produces method_definition inside a class body, so parsing it alone gives
    an ERROR tree in which the parameters are lost and the method name reads as a callee.
    Reparse those inside a class wrapper.
    """
    tree = parse_source(source)
    if tree.root_node.has_error:
        return parse_source("class __W {\n" + source + "\n}")
    return tree
