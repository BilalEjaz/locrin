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
    """Parse the source of a single function, whatever shape it arrives in.

    extract.FunctionRecord.source is the exact node text, so two shapes do not parse as a
    standalone TypeScript program:

    * a method arrives as a bare method body ("async load(id) { ... }"), and the grammar
      only produces method_definition inside a class body;
    * a .tsx component returns JSX, which the TypeScript grammar cannot parse.

    Try the plain source under both grammars first, and only then the class wrapper, so a
    JSX component is never given a class it does not need. Return the first tree that
    parses without an error, else the last one tried.
    """
    wrapped = "class __W {\n" + source + "\n}"
    attempts = ((source, False), (source, True), (wrapped, False), (wrapped, True))
    tree = None
    for text, tsx in attempts:
        tree = parse_source(text, tsx=tsx)
        if not tree.root_node.has_error:
            return tree
    return tree
