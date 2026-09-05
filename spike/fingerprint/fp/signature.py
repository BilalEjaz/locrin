from dataclasses import dataclass

from tree_sitter import Node

from fp.parse import parse_source


@dataclass(frozen=True)
class Signature:
    param_count: int
    callees: frozenset[str]
    returns_value: bool
    is_async: bool


def _first_function(root: Node) -> Node | None:
    stack = [root]
    while stack:
        n = stack.pop()
        if n.type in {"function_declaration", "generator_function_declaration", "method_definition",
                      "arrow_function", "function_expression", "function"}:
            return n
        stack.extend(reversed(n.children))
    return None


def _root_for(source: str) -> Node:
    """Root node to read a single function from.

    extract.FunctionRecord.source is the exact node text, so a method arrives as a bare
    method body ("async load(id) { ... }"). That is not valid standalone TypeScript: the
    grammar only produces method_definition inside a class body, so parsing it alone gives
    an ERROR tree in which the method name reads as a callee and the parameters are lost.
    Reparse those inside a class wrapper.
    """
    root = parse_source(source).root_node
    if _first_function(root) is None and root.has_error:
        wrapped = parse_source("class __FpWrapper__ {\n" + source + "\n}").root_node
        if _first_function(wrapped) is not None and not wrapped.has_error:
            return wrapped
    return root


def _callee_name(call: Node) -> str | None:
    fn = call.child_by_field_name("function") or call.child_by_field_name("constructor")
    if fn is None:
        return None
    if fn.type == "member_expression":
        prop = fn.child_by_field_name("property")
        return prop.text.decode("utf8") if prop is not None else None
    if fn.type in {"identifier", "type_identifier"}:
        return fn.text.decode("utf8")
    return None


def signature_of(source: str) -> Signature:
    root = _root_for(source)
    fn = _first_function(root) or root
    params = fn.child_by_field_name("parameters")
    param_count = 0
    if params is not None:
        param_count = sum(1 for c in params.children if c.is_named and c.type != "comment")
    elif fn.type == "arrow_function":
        single = fn.child_by_field_name("parameter")
        param_count = 1 if single is not None else 0

    callees: set[str] = set()
    returns_value = False
    is_async = any(c.type == "async" for c in fn.children)
    stack = list(fn.children)
    while stack:
        n = stack.pop()
        if n.type in {"call_expression", "new_expression"}:
            name = _callee_name(n)
            if name:
                callees.add(name)
        if n.type == "return_statement" and any(c.is_named for c in n.children):
            returns_value = True
        stack.extend(n.children)
    if fn.type == "arrow_function":
        body = fn.child_by_field_name("body")
        if body is not None and body.type != "statement_block":
            returns_value = True
    return Signature(param_count, frozenset(callees), returns_value, is_async)


def _jaccard(a: frozenset[str], b: frozenset[str]) -> float:
    if not a and not b:
        return 1.0
    if not a or not b:
        return 0.0
    return len(a & b) / len(a | b)


def similarity(a: Signature, b: Signature) -> float:
    callee_score = _jaccard(a.callees, b.callees)
    param_score = 1.0 if a.param_count == b.param_count else 0.0
    ret_score = 1.0 if a.returns_value == b.returns_value else 0.0
    async_score = 1.0 if a.is_async == b.is_async else 0.0
    return 0.6 * callee_score + 0.2 * param_score + 0.1 * ret_score + 0.1 * async_score


def gate(a: Signature, b: Signature) -> bool:
    return a.param_count == b.param_count or _jaccard(a.callees, b.callees) >= 0.5
