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
