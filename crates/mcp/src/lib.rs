//! The MCP protocol layer: newline-delimited JSON-RPC 2.0 over a reader and a
//! writer, and nothing else. It knows the handshake, the four methods a tool
//! server owes a client, and the error codes; it knows nothing about the
//! engine. The CLI supplies the tools through [`Handler`].
//!
//! The transport is stdio, so stdout carries messages and only messages: one
//! compact JSON object per line, no embedded newlines. Anything the server
//! wants to say to a human belongs on stderr.

use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};

use serde_json::{json, Value};

/// One tool as the client sees it in `tools/list`.
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// A JSON Schema object for the tool's arguments.
    pub input_schema: Value,
}

/// A tool that ran and has something wrong to report. Becomes a result with
/// `isError: true`, which the model sees; a JSON-RPC error is reserved for a
/// request the server could not understand.
pub struct ToolError(pub String);

/// The tools this server offers. Implemented by the CLI, which owns the engine.
pub trait Handler {
    fn tools(&self) -> Vec<ToolSpec>;
    fn call(&mut self, name: &str, args: &Value) -> Result<String, ToolError>;
}

/// Every protocol revision this server can speak, newest first. An `initialize`
/// that asks for one of these gets it back; anything else is answered with the
/// newest and the client decides whether it can live with that.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Serves one connection: reads newline-delimited JSON-RPC from `reader`, writes
/// one message per line to `writer`, flushing after each, until EOF. Never
/// panics on input and never abandons the session over one bad message: a line
/// that is not JSON, or not even UTF-8, is answered with a `-32700` parse error
/// and the loop continues, and a blank line is skipped without an answer. Only a
/// reader or writer that has actually broken comes back as `Err`. Returns when
/// stdin closes.
pub fn serve<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
    handler: &mut dyn Handler,
    name: &str,
    version: &str,
) -> anyhow::Result<()> {
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        // Bytes rather than `lines()`, which turns a line that is not UTF-8
        // into a read error and ends the session. Such a line is a message the
        // server cannot understand, which the protocol has an answer for, and
        // taking the bytes first leaves the stream in sync for the next one.
        // A read that returns nothing is EOF, the one way out of this loop.
        if reader.read_until(b'\n', &mut buffer)? == 0 {
            return Ok(());
        }
        let Ok(line) = std::str::from_utf8(&buffer) else {
            write_message(&mut writer, &error(Value::Null, -32700, "parse error"))?;
            continue;
        };
        // A bare newline is a separator, not a message: a client that ended its
        // last line and nothing more is owed silence, not an error it never
        // asked for and would have to explain away.
        if line.trim().is_empty() {
            continue;
        }
        if let Some(message) = handle(line, handler, name, version) {
            write_message(&mut writer, &message)?;
        }
    }
}

/// One message, one line, flushed. The client is waiting on this line before it
/// sends the next one, so a buffered response is a deadlock.
fn write_message<W: Write>(writer: &mut W, message: &Value) -> anyhow::Result<()> {
    writeln!(writer, "{}", serde_json::to_string(message)?)?;
    writer.flush()?;
    Ok(())
}

/// One message in, at most one message out. Public so the tests can drive it
/// without a pipe.
pub fn handle(line: &str, handler: &mut dyn Handler, name: &str, version: &str) -> Option<Value> {
    let request: Value = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(_) => return Some(error(Value::Null, -32700, "parse error")),
    };

    let id = request.get("id").cloned().unwrap_or(Value::Null);

    // Checked before the id, because a message with no method is malformed
    // rather than a notification, and a client that sent one is owed the
    // reason whether or not it left room for an answer.
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return Some(error(id, -32600, "invalid request"));
    };

    // No id means a notification, and the spec forbids answering one at all,
    // whatever the method and however little sense it made.
    if id.is_null() {
        return None;
    }

    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let agreed = match asked {
                Some(v) if PROTOCOL_VERSIONS.contains(&v) => v,
                _ => PROTOCOL_VERSIONS[0],
            };
            json!({
                "protocolVersion": agreed,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": name, "version": version},
            })
        }
        "ping" => json!({}),
        "tools/list" => {
            let tools: Vec<Value> = handler
                .tools()
                .into_iter()
                .map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.input_schema}))
                .collect();
            json!({ "tools": tools })
        }
        "tools/call" => match call(handler, &params) {
            Ok(result) => result,
            Err((code, message)) => return Some(error(id, code, &message)),
        },
        _ => return Some(error(id, -32601, "method not found")),
    };

    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// Runs one `tools/call`. `Err` is for the requests the protocol rejects; a tool
/// that ran and failed comes back as an `Ok` result carrying `isError`.
fn call(handler: &mut dyn Handler, params: &Value) -> Result<Value, (i64, String)> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((-32602, "invalid params".into()));
    };
    // Anything that is not an object, `null` included, is treated as no
    // arguments: a tool validates its own schema and says so in its own words.
    let args = params.get("arguments").filter(|a| a.is_object()).cloned().unwrap_or_else(|| json!({}));

    if !handler.tools().iter().any(|t| t.name == name) {
        return Err((-32602, format!("unknown tool: {name}")));
    }

    // A tool that panics must not take the server down with it: the client
    // would see the pipe close mid-session with no idea which call did it.
    let outcome = catch_unwind(AssertUnwindSafe(|| handler.call(name, &args)));
    Ok(match outcome {
        Ok(Ok(text)) => json!({"content": [{"type": "text", "text": text}]}),
        Ok(Err(ToolError(text))) => json!({"content": [{"type": "text", "text": text}], "isError": true}),
        Err(payload) => {
            // The client is told "internal engine failure" and no more, because
            // a panic message is not something a model can act on. Somebody
            // debugging the server needs the opposite, and the binary silences
            // the default panic hook, so this line is the only trace the panic
            // leaves anywhere. Stderr, because stdout is the protocol.
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            eprintln!("warning: tool {name} panicked: {message}");
            json!({"content": [{"type": "text", "text": "internal engine failure"}], "isError": true})
        }
    })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Cursor;

    /// Three tools, which is every outcome the dispatch rules care about: a call
    /// that works, a call the tool itself refuses, and a call that blows up.
    struct Fake;

    impl Handler for Fake {
        fn tools(&self) -> Vec<ToolSpec> {
            vec![
                ToolSpec {
                    name: "echo",
                    description: "returns its text argument",
                    input_schema: json!({"type": "object", "properties": {"text": {"type": "string"}}}),
                },
                ToolSpec {
                    name: "fail",
                    description: "always reports a tool error",
                    input_schema: json!({"type": "object"}),
                },
                ToolSpec { name: "boom", description: "panics", input_schema: json!({"type": "object"}) },
            ]
        }

        fn call(&mut self, name: &str, args: &Value) -> Result<String, ToolError> {
            match name {
                "echo" => Ok(args["text"].as_str().unwrap_or_default().to_string()),
                "fail" => Err(ToolError("the tool said no".into())),
                "boom" => panic!("the tool exploded"),
                other => Err(ToolError(format!("no tool called {other}"))),
            }
        }
    }

    /// One request in, the response out. Every test that is not about the pipe
    /// goes through here.
    fn ask(line: &str) -> Value {
        handle(line, &mut Fake, "locrin", "0.1.0").expect("a request with an id gets a response")
    }

    #[test]
    fn initialize_echoes_a_supported_version_and_declares_tools() {
        let r = ask(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
        );
        assert_eq!(r["id"], json!(1));
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["capabilities"]["tools"], json!({}));
        assert_eq!(r["result"]["serverInfo"]["name"], "locrin");
        assert_eq!(r["result"]["serverInfo"]["version"], "0.1.0");
    }

    #[test]
    fn initialize_falls_back_to_the_newest_version_for_an_unknown_one() {
        let r = ask(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#);
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_response() {
        for line in [
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
            r#"{"jsonrpc":"2.0","method":"whatever/else"}"#,
        ] {
            assert!(handle(line, &mut Fake, "locrin", "0.1.0").is_none(), "answered a notification: {line}");
        }
    }

    #[test]
    fn an_explicit_null_id_is_a_notification_too() {
        // JSON-RPC 2.0 discourages a null id and MCP forbids one on a request,
        // so there is no correlation to answer even though the key is present.
        let line = r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#;
        assert!(handle(line, &mut Fake, "locrin", "0.1.0").is_none(), "answered a null id: {line}");
    }

    #[test]
    fn a_message_with_neither_method_nor_id_is_an_invalid_request() {
        // Malformed beats notification: a message with no method never asked
        // for anything, so it is owed the reason rather than silence.
        let r = ask(r#"{"jsonrpc":"2.0"}"#);
        assert_eq!(r["id"], Value::Null);
        assert_eq!(r["error"]["code"], -32600);
        assert_eq!(r["error"]["message"], "invalid request");
    }

    #[test]
    fn tools_list_describes_every_tool() {
        let r = ask(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#);
        let tools = r["result"]["tools"].as_array().expect("a tools array");
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["name"], "echo");
        assert_eq!(tools[0]["description"], "returns its text argument");
        assert_eq!(tools[0]["inputSchema"]["properties"]["text"]["type"], "string");
    }

    #[test]
    fn tools_call_wraps_text() {
        let r = ask(
            r#"{"jsonrpc":"2.0","id":"a","method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"}}}"#,
        );
        assert_eq!(r["id"], json!("a"));
        assert_eq!(r["result"]["content"][0]["type"], "text");
        assert_eq!(r["result"]["content"][0]["text"], "hi");
        assert!(r["result"].get("isError").is_none());
    }

    #[test]
    fn tool_error_is_is_error_not_a_protocol_error() {
        let r = ask(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fail","arguments":{}}}"#);
        assert!(r.get("error").is_none(), "a failing tool is not a protocol error");
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(r["result"]["content"][0]["text"], "the tool said no");
    }

    #[test]
    fn a_panicking_tool_is_is_error_and_the_next_call_still_works() {
        let mut fake = Fake;
        let boom = handle(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"boom","arguments":{}}}"#,
            &mut fake,
            "locrin",
            "0.1.0",
        )
        .expect("a panicking tool still answers");
        assert_eq!(boom["result"]["isError"], true);
        assert_eq!(boom["result"]["content"][0]["text"], "internal engine failure");

        let after = handle(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{"text":"still here"}}}"#,
            &mut fake,
            "locrin",
            "0.1.0",
        )
        .expect("the server is still up");
        assert_eq!(after["result"]["content"][0]["text"], "still here");
    }

    /// The panic's own message goes to stderr, which `handle` cannot show a
    /// test: the libtest harness captures it and hands it back only under
    /// `--nocapture`. What is asserted here is that logging it changed nothing
    /// the client sees, and the line itself is read by running this test with
    /// `--nocapture`.
    #[test]
    fn a_panicking_tool_is_still_is_error_when_the_panic_is_logged() {
        let result = call(&mut Fake, &json!({"name": "boom", "arguments": {}})).expect("not a protocol error");
        assert_eq!(result["isError"], true, "{result}");
        assert_eq!(result["content"][0]["text"], "internal engine failure", "{result}");
    }

    #[test]
    fn unknown_tool_is_invalid_params() {
        let r = ask(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#);
        assert_eq!(r["error"]["code"], -32602);
        assert_eq!(r["error"]["message"], "unknown tool: nope");
    }

    #[test]
    fn tools_call_without_a_name_is_invalid_params() {
        let r = ask(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{}}"#);
        assert_eq!(r["error"]["code"], -32602);
        assert_eq!(r["error"]["message"], "invalid params");
    }

    #[test]
    fn tools_call_without_arguments_calls_the_tool_with_an_empty_object() {
        // An absent `arguments` is not a protocol error: the tool is called
        // with `{}` and says in its own words what a missing key means to it.
        let r = ask(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"echo"}}"#);
        assert!(r.get("error").is_none(), "a missing arguments object is not a protocol error");
        assert_eq!(r["result"]["content"][0]["text"], "");
        assert!(r["result"].get("isError").is_none());
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let r = ask(r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#);
        assert_eq!(r["id"], json!(4));
        assert_eq!(r["error"]["code"], -32601);
        assert_eq!(r["error"]["message"], "method not found");
    }

    #[test]
    fn garbage_is_a_parse_error_with_null_id() {
        let r = ask("not json at all {");
        assert_eq!(r["jsonrpc"], "2.0");
        assert_eq!(r["id"], Value::Null);
        assert_eq!(r["error"]["code"], -32700);
        assert_eq!(r["error"]["message"], "parse error");

        // A well formed message that is not a request is a different failure.
        let r = ask(r#"{"jsonrpc":"2.0","id":9,"method":42}"#);
        assert_eq!(r["id"], json!(9));
        assert_eq!(r["error"]["code"], -32600);
        assert_eq!(r["error"]["message"], "invalid request");
    }

    #[test]
    fn serve_runs_a_whole_conversation_over_a_pipe() {
        let input = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":"three","method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#,
        ]
        .join("\n");
        let mut out: Vec<u8> = Vec::new();
        serve(Cursor::new(input), &mut out, &mut Fake, "locrin", "0.1.0").expect("the pipe runs to EOF");

        let out = String::from_utf8(out).expect("utf-8 out");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 4, "one response per request and none for the notification");
        let ids: Vec<Value> = lines
            .iter()
            .map(|l| serde_json::from_str::<Value>(l).expect("every line parses as one message")["id"].clone())
            .collect();
        assert_eq!(ids, vec![json!(1), json!(2), json!("three"), json!(4)]);
        let ping: Value = serde_json::from_str(lines[3]).expect("the ping response parses");
        assert_eq!(ping["result"], json!({}));
    }

    #[test]
    fn serve_answers_a_line_that_is_not_utf8_and_keeps_the_session() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        input.push(b'\n');
        // 0xFF cannot appear in UTF-8 at all, so this is a line no decoder will
        // ever hand back as a string, not merely one that is not JSON.
        input.extend_from_slice(&[b'{', 0xFF, b'}']);
        input.push(b'\n');
        input.extend_from_slice(br#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#);
        input.push(b'\n');

        let mut out: Vec<u8> = Vec::new();
        serve(Cursor::new(input), &mut out, &mut Fake, "locrin", "0.1.0")
            .expect("a line the server cannot read is not a broken stream");

        let out = String::from_utf8(out).expect("utf-8 out");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "the bad line is answered and the session carries on");
        let bad: Value = serde_json::from_str(lines[1]).expect("the parse error parses");
        assert_eq!(bad["id"], Value::Null);
        assert_eq!(bad["error"]["code"], -32700);
        let after: Value = serde_json::from_str(lines[2]).expect("the next response parses");
        assert_eq!(after["id"], json!(2), "the stream stayed in sync past the bad line");
    }

    #[test]
    fn serve_skips_blank_lines_in_silence() {
        // A keepalive newline or a trailing separator is not a message, and an
        // error nobody asked for is the client's problem to explain away.
        for separator in ["\n", "\r\n"] {
            let input = [
                r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
                "",
                r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
                "",
                "",
            ]
            .join(separator);
            let mut out: Vec<u8> = Vec::new();
            serve(Cursor::new(input), &mut out, &mut Fake, "locrin", "0.1.0").expect("the pipe runs to EOF");

            let out = String::from_utf8(out).expect("utf-8 out");
            let lines: Vec<&str> = out.lines().collect();
            assert_eq!(lines.len(), 2, "a blank line answered, separator {separator:?}");
            let ids: Vec<Value> = lines
                .iter()
                .map(|l| serde_json::from_str::<Value>(l).expect("every line parses as one message")["id"].clone())
                .collect();
            assert_eq!(ids, vec![json!(1), json!(2)]);
        }
    }
}
