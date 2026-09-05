from pathlib import Path
from fp.extract import extract_functions

FIX = Path(__file__).parent / "fixtures" / "basic.ts"


def records():
    return extract_functions(str(FIX), FIX.read_text(encoding="utf8"))


def test_finds_declaration_arrow_and_method():
    names = {r.name for r in records()}
    assert {"add", "multiply", "divide"} <= names


def test_records_have_lines_and_source():
    add = next(r for r in records() if r.name == "add")
    assert add.start_line == 1
    assert add.end_line == 5
    assert add.source.startswith("function add(")
    assert add.id == f"{add.file}:1:add"


def test_tiny_arrow_is_still_extracted_here():
    # size filtering happens in normalize/index, not in extract
    assert any(r.name == "tiny" for r in records())
