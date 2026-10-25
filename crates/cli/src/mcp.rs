//! `simplefp mcp`: a Model Context Protocol server on stdio (JSON-RPC 2.0, one message
//! per line) so agents can drive SimpleFP. Every tool runs the matching CLI subcommand
//! of this same binary, so agents get exactly the CLI's validation, outputs and files.
//! Arguments are checked here first (unknown fields rejected, sizes bounded) and passed
//! as `--flag=value` with positionals after `--`, so no value can become a flag.

use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";
const MAX_STR: usize = 4096;
const MAX_LIST: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    block: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShowPreset {
    id: String,
    #[serde(default)]
    set: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Analyze {
    block: String,
    preset: Option<String>,
    #[serde(default)]
    epoc: Vec<String>,
    #[serde(default)]
    epoc_offset: Vec<String>,
    events_csv: Option<String>,
    out: Option<String>,
    #[serde(default)]
    set: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cohort {
    manifest: String,
    out: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Track {
    block: String,
    width: Option<u32>,
    roi: Option<[f64; 4]>,
    light_animal: Option<bool>,
    out: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Train {
    blocks: Vec<String>,
    labels: Vec<String>,
    behavior: String,
    width: Option<u32>,
    roi: Option<[f64; 4]>,
    out: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Propose {
    block: String,
    model: String,
    roi: Option<[f64; 4]>,
    out: Option<String>,
}

fn s(v: &str) -> Result<String, String> {
    if v.is_empty() || v.len() > MAX_STR || v.contains('\0') {
        return Err(format!("strings must be 1–{MAX_STR} bytes without NUL"));
    }
    Ok(v.to_string())
}

fn list(v: &[String]) -> Result<Vec<String>, String> {
    if v.len() > MAX_LIST {
        return Err(format!("at most {MAX_LIST} items per list"));
    }
    v.iter().map(|x| s(x)).collect()
}

fn flag(name: &str, v: &str) -> Result<String, String> {
    Ok(format!("--{name}={}", s(v)?))
}

/// `--set` overrides: dotted keys (`preprocess.lowpass_hz`) and scalar values, validated again by the preset validator.
fn sets(m: &BTreeMap<String, Value>) -> Result<Vec<String>, String> {
    if m.len() > MAX_LIST {
        return Err(format!("at most {MAX_LIST} overrides"));
    }
    m.iter()
        .map(|(k, v)| {
            if k.is_empty() || k.len() > 128 || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                return Err(format!("override key '{k}' must be a dotted name like preprocess.lowpass_hz"));
            }
            let v = match v {
                Value::String(x) => s(x)?,
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => return Err(format!("override '{k}' must be a string, number or boolean")),
            };
            Ok(format!("--set={k}={v}"))
        })
        .collect()
}

fn roi(r: &Option<[f64; 4]>) -> Vec<String> {
    r.map(|r| vec![format!("--roi={},{},{},{}", r[0], r[1], r[2], r[3])]).unwrap_or_default()
}

fn opt(name: &str, v: &Option<String>) -> Result<Vec<String>, String> {
    v.as_deref().map(|v| flag(name, v)).transpose().map(|o| o.into_iter().collect())
}

fn args<T: for<'de> Deserialize<'de>>(a: &Value) -> Result<T, String> {
    serde_json::from_value(a.clone()).map_err(|e| format!("invalid arguments: {e}"))
}

/// Tool name → CLI argv (after the binary name).
fn argv(name: &str, a: &Value) -> Result<Vec<String>, String> {
    let a = if a.is_null() { &json!({}) } else { a };
    let mut v: Vec<String> = vec![];
    match name {
        "inspect_block" => {
            let p: Block = args(a)?;
            v.extend(["inspect".into(), "--".into(), s(&p.block)?]);
        }
        "list_presets" => {
            if a.as_object().is_some_and(|o| !o.is_empty()) {
                return Err("list_presets takes no arguments".into());
            }
            v.push("presets".into());
        }
        "show_preset" => {
            let p: ShowPreset = args(a)?;
            v.push("presets".into());
            v.extend(sets(&p.set)?);
            v.extend(["--".into(), s(&p.id)?]);
        }
        "analyze" => {
            let p: Analyze = args(a)?;
            v.push("run".into());
            v.extend(opt("preset", &p.preset)?);
            for e in list(&p.epoc)? {
                v.push(flag("epoc", &e)?);
            }
            for e in list(&p.epoc_offset)? {
                v.push(flag("epoc-offset", &e)?);
            }
            v.extend(opt("events", &p.events_csv)?);
            v.extend(opt("out", &p.out)?);
            v.extend(sets(&p.set)?);
            v.extend(["--".into(), s(&p.block)?]);
        }
        "cohort" => {
            let p: Cohort = args(a)?;
            v.push("cohort".into());
            v.extend(opt("out", &p.out)?);
            v.extend(["--".into(), s(&p.manifest)?]);
        }
        "track" => {
            let p: Track = args(a)?;
            v.push("track".into());
            v.extend(p.width.map(|w| format!("--width={w}")));
            v.extend(roi(&p.roi));
            if p.light_animal == Some(true) {
                v.push("--light-animal".into());
            }
            v.extend(opt("out", &p.out)?);
            v.extend(["--".into(), s(&p.block)?]);
        }
        "train_detector" => {
            let p: Train = args(a)?;
            if p.blocks.is_empty() || p.blocks.len() != p.labels.len() {
                return Err("give one labels CSV per block".into());
            }
            v.push("classify-train".into());
            for l in list(&p.labels)? {
                v.push(flag("labels", &l)?);
            }
            v.push(flag("behavior", &p.behavior)?);
            v.extend(p.width.map(|w| format!("--width={w}")));
            v.extend(roi(&p.roi));
            v.push(flag("out", &p.out)?);
            v.push("--".into());
            v.extend(list(&p.blocks)?);
        }
        "propose_bouts" => {
            let p: Propose = args(a)?;
            v.push("classify-predict".into());
            v.push(flag("model", &p.model)?);
            v.extend(roi(&p.roi));
            v.extend(opt("out", &p.out)?);
            v.extend(["--".into(), s(&p.block)?]);
        }
        _ => return Err(format!("unknown tool '{name}'")),
    }
    Ok(v)
}

fn tools() -> Value {
    let path = json!({"type": "string", "description": "Absolute path"});
    let set = json!({"type": "object", "description": "Preset overrides by dotted key, e.g. {\"preprocess.lowpass_hz\": 6}. Validated like the app's Easy mode.", "additionalProperties": {"type": ["string", "number", "boolean"]}});
    let roi = json!({"type": "array", "items": {"type": "number", "minimum": 0, "maximum": 1}, "minItems": 4, "maxItems": 4, "description": "Arena as frame fractions [x0, y0, x1, y1]; default auto-detect"});
    let tool = |name: &str, desc: &str, props: Value, req: &[&str]| json!({"name": name, "description": desc, "inputSchema": {"type": "object", "properties": props, "required": req, "additionalProperties": false}});
    json!([
        tool("inspect_block", "List the streams and epocs (event stores) of a TDT block folder.", json!({"block": path}), &["block"]),
        tool("list_presets", "List built-in analysis presets.", json!({}), &[]),
        tool("show_preset", "Print a preset as TOML, optionally with overrides applied and validated (use to check a parameter change before analyzing).", json!({"id": {"type": "string", "description": "Built-in id or path to a .toml preset"}, "set": set}), &["id"]),
        tool("analyze", "Preprocess a TDT block (filtering, bleaching and motion correction, ΔF/F) and run peri-event analysis with bootstrap CIs. Writes CSV/JSON results to `out` (default <block>/simplefp) and returns a QC summary.",
            json!({"block": path, "preset": {"type": "string", "description": "Built-in id or .toml path; default grab-ach3.0-tdt"}, "epoc": {"type": "array", "items": {"type": "string"}, "description": "Epoc stores whose onsets are events"}, "epoc_offset": {"type": "array", "items": {"type": "string"}, "description": "Epoc stores whose offsets are events"}, "events_csv": {"type": "string", "description": "CSV with header event,time_s on the block clock"}, "out": path, "set": set}), &["block"]),
        tool("cohort", "Group analysis from a cohort manifest (animal is the unit; permutation tests with BH correction).", json!({"manifest": path, "out": path}), &["manifest"]),
        tool("track", "Track the animal in the block's Cam1 video (centroid, speed, immobility on the TDT clock; needs ffmpeg).", json!({"block": path, "width": {"type": "integer", "minimum": 64, "maximum": 640}, "roi": roi, "light_animal": {"type": "boolean"}, "out": path}), &["block"]),
        tool("train_detector", "Train a behavior detector from human-scored bouts (one CSV behavior,start_s,end_s per block) and report held-out agreement, the v1 baseline and a learning curve. Slow: tens of seconds per session.",
            json!({"blocks": {"type": "array", "items": path, "minItems": 1}, "labels": {"type": "array", "items": path, "minItems": 1}, "behavior": {"type": "string"}, "width": {"type": "integer", "minimum": 64, "maximum": 640}, "roi": roi, "out": {"type": "string", "description": "Model file path (.json)"}}), &["blocks", "labels", "behavior", "out"]),
        tool("propose_bouts", "Propose behavior bouts in a block with a trained detector. Proposals are written for human review and never count as scored data until accepted in the app.", json!({"block": path, "model": path, "roi": roi, "out": path}), &["block", "model"]),
    ])
}

fn call(name: &str, a: &Value) -> Value {
    let result = argv(name, a).and_then(|v| {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let o = std::process::Command::new(exe).args(&v).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
        let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&o.stderr));
        Ok((text, o.status.success()))
    });
    let (text, ok) = result.unwrap_or_else(|e| (format!("error: {e}"), false));
    json!({"content": [{"type": "text", "text": text}], "isError": !ok})
}

/// One JSON-RPC message in, at most one out (notifications get no reply).
fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let p = &msg["params"];
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => Ok(json!({
            "protocolVersion": p["protocolVersion"].as_str().filter(|v| *v == PROTOCOL || *v == "2025-03-26" || *v == "2024-11-05").unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "simplefp", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "SimpleFP analyzes TDT fiber photometry recordings. Start with inspect_block, check parameter changes with show_preset, then analyze. Model proposals need human review in the app."
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => match p["name"].as_str() {
            Some(n) => Ok(call(n, &p["arguments"])),
            None => Err((-32602, "tools/call needs a tool name".to_string())),
        },
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

    #[test]
    fn arguments_cannot_become_flags_and_unknown_fields_fail() {
        let v = argv("analyze", &json!({"block": "--out=/tmp/x", "set": {"preprocess.lowpass_hz": 6}})).unwrap();
        assert_eq!(v, ["run", "--set=preprocess.lowpass_hz=6", "--", "--out=/tmp/x"]);
        assert!(argv("analyze", &json!({"block": "/b", "shell": "rm"})).is_err());
        assert!(argv("analyze", &json!({"block": "/b", "set": {"a;b": 1}})).is_err());
        assert!(argv("train_detector", &json!({"blocks": ["/a"], "labels": [], "behavior": "x", "out": "/m"})).is_err());
        assert!(argv("nope", &json!({})).is_err());
        assert!(handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        assert_eq!(handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "bogus"})).unwrap()["error"]["code"], -32601);
        assert_eq!(tools().as_array().unwrap().len(), 8);
    }
}
