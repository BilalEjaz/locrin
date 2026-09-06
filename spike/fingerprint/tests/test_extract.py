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
    assert add.id == f"{add.file}:1:7:add"


def test_tiny_arrow_is_still_extracted_here():
    # size filtering happens in normalize/index, not in extract
    assert any(r.name == "tiny" for r in records())


def test_only_named_nodes_are_extracted():
    rs = records()
    assert len(rs) == 4
    assert {r.name for r in rs} == {"add", "multiply", "divide", "tiny"}
    assert all(r.source != "function" for r in rs)


SAME_LINE = Path(__file__).parent / "fixtures" / "same_line.ts"


def same_line_records():
    return extract_functions(str(SAME_LINE), SAME_LINE.read_text(encoding="utf8"))


def test_same_line_arrows_get_distinct_ids():
    rs = same_line_records()
    anons = [r for r in rs if r.name == "<anon>"]
    assert len(anons) == 2
    # both callbacks open on the same line, so line alone cannot separate them
    assert anons[0].start_line == anons[1].start_line == 4
    # both bodies span multiple lines
    assert all(r.end_line > r.start_line for r in anons)
    assert anons[0].id != anons[1].id
    assert len({r.id for r in rs}) == len(rs)
