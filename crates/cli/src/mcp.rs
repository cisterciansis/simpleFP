//! `simplefp mcp`: a Model Context Protocol server on stdio (JSON-RPC 2.0, one message per line). Tools, schemas
//! and results come from the registry (`simplefp::tools`) and run in-process, so agents get exactly the CLI's
//! validation and outputs. Arguments are size-bounded here, then strictly parsed by the tool (unknown fields fail).

use serde_json::{Value, json};
use simplefp::tools::{self, TOOLS};
use std::io::{BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

const PROTOCOL: &str = "2025-06-18";
const MAX_LINE: usize = 1 << 20;
const MAX_STR: usize = 4096;
const MAX_LIST: usize = 64;
const MAX_DEPTH: usize = 6;

/// Bound untrusted arguments before any tool sees them: string length and NUL, list and object size, nesting.
fn bounded(v: &Value, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("arguments nest too deeply".into());
    }
    match v {
        Value::String(s) if s.len() > MAX_STR || s.contains('\0') => Err(format!("strings must be at most {MAX_STR} bytes without NUL")),
        Value::Array(a) if a.len() > MAX_LIST => Err(format!("at most {MAX_LIST} items per list")),
        Value::Object(o) if o.len() > MAX_LIST || o.keys().any(|k| k.len() > 128) => Err(format!("at most {MAX_LIST} keys of up to 128 bytes")),
        Value::Array(a) => a.iter().try_for_each(|x| bounded(x, depth + 1)),
        Value::Object(o) => o.values().try_for_each(|x| bounded(x, depth + 1)),
        _ => Ok(()),
    }
}

fn tool_list() -> Value {
    let list: Vec<Value> = TOOLS
        .iter()
        .map(|t| {
            let mut schema = (t.schema)();
            let description = schema.as_object_mut().and_then(|o| o.remove("description"));
            json!({"name": t.name, "description": description, "inputSchema": schema})
        })
        .collect();
    json!({"tools": list})
}

/// Run a tool. Tool failures are results with `isError` (the agent can read and fix them); an unknown tool is a
/// protocol error.
fn call(p: &Value) -> Result<Value, (i64, String)> {
    let name = p["name"].as_str().ok_or((-32602, "tools/call needs a tool name".to_string()))?;
    let tool = tools::find(name).ok_or((-32602, format!("unknown tool '{name}'")))?;
    let args = if p["arguments"].is_null() { json!({}) } else { p["arguments"].clone() };
    let result = bounded(&args, 0).and_then(|()| catch_unwind(AssertUnwindSafe(|| (tool.call)(args))).unwrap_or_else(|_| Err("internal error (panic); details on stderr".into())));
    Ok(match result {
        Ok(v) => json!({"content": [{"type": "text", "text": v.to_string()}], "structuredContent": v, "isError": false}),
        Err(e) => json!({"content": [{"type": "text", "text": format!("error: {e}")}], "isError": true}),
    })
}

/// One JSON-RPC message in, at most one out (notifications get no reply).
fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let p = &msg["params"];
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => Ok(json!({
            "protocolVersion": p["protocolVersion"].as_str().filter(|v| *v == PROTOCOL || *v == "2025-03-26" || *v == "2024-11-05").unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "simplefp", "version": simplefp::VERSION},
            "instructions": "SimpleFP analyzes TDT fiber photometry recordings. Start with inspect, check parameter changes with presets {id, set}, then run. `demo` writes a synthetic block to try things on. Model proposals need human review in the app."
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tool_list()),
        "tools/call" => call(p),
        m => Err((-32601, format!("method not found: {m}"))),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    })
}

pub fn serve() -> Result<(), String> {
    let stdout = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            _ if line.len() > MAX_LINE => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "message too large"}})),
            Ok(m) => handle(&m),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}})),
        };
        if let Some(r) = reply {
            let mut out = stdout.lock();
            writeln!(out, "{r}").and_then(|_| out.flush()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(method: &str, params: Value) -> Value {
        handle(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})).unwrap()
    }

    #[test]
    fn protocol_and_bounds() {
        assert!(handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        assert_eq!(rpc("bogus", json!({}))["error"]["code"], -32601);
        assert_eq!(rpc("tools/call", json!({"name": "nope"}))["error"]["code"], -32602);
        assert_eq!(rpc("tools/list", json!({}))["result"]["tools"].as_array().unwrap().len(), TOOLS.len());
        let r = rpc("tools/call", json!({"name": "run", "arguments": {"block": "/b", "shell": "rm"}}));
        assert_eq!(r["result"]["isError"], true);
        let r = rpc("tools/call", json!({"name": "inspect", "arguments": {"block": "x".repeat(MAX_STR + 1)}}));
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("at most"));
        let r = rpc("tools/call", json!({"name": "presets"}));
        assert!(r["result"]["structuredContent"]["presets"].as_array().is_some_and(|p| !p.is_empty()));
    }
}
