//! `clementine mcp`: the engine as a Model Context Protocol server, so AI
//! agents (Claude Code, Claude Desktop…) can convert files. JSON-RPC 2.0,
//! one message per line on stdin/stdout.

use anyhow::Result;
use clementine_core::{catalog, operations, run, run_recipe, Category, Options};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// Protocol version answered when the client asks for one we don't know.
const LATEST: &str = "2025-06-18";
const KNOWN: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

pub fn serve() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg),
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Answer one message; notifications (no `id`) get no answer.
fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match msg.get("method").and_then(Value::as_str).unwrap_or("") {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or(LATEST);
            let version = if KNOWN.contains(&asked) {
                asked
            } else {
                LATEST
            };
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "clementine", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Local, offline file conversion. Call list_operations with the \
                    files to see what can be done, then run_operation (one step) or run_recipe \
                    (several steps chained). Pass absolute paths. Results are written next to \
                    the originals unless out_dir is given.",
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(name, &args) {
                Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                // Tool failures are results the model can read, not protocol errors.
                Err(e) => json!({
                    "content": [{ "type": "text", "text": format!("Error: {e:#}") }],
                    "isError": true,
                }),
            }
        }
        other => return Some(error(id, -32601, &format!("method not found: {other}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tools() -> Value {
    let files = json!({
        "type": "array",
        "items": { "type": "string" },
        "description": "Absolute paths of the input files.",
        "minItems": 1,
    });
    let options = json!({
        "out_dir": { "type": "string", "description": "Folder for the results. Default: next to each original." },
        "quality": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Lossy quality, default 85." },
        "video_preset": { "type": "string", "enum": ["standard", "web", "max"], "description": "Video encoding: standard (follows quality), web (≤ 1080p, lighter) or max (near lossless)." },
        "target_mb": { "type": "number", "description": "Size tool:fit-size aims under, in MB. Default 10." },
    });
    let with = |extra: Value, required: &[&str]| {
        let mut props = options.clone();
        for (k, v) in extra.as_object().unwrap() {
            props[k] = v.clone();
        }
        json!({ "type": "object", "properties": props, "required": required })
    };
    json!([
        {
            "name": "list_operations",
            "description": "List the conversions and tools available for these files, with the operation ids to pass to run_operation or run_recipe.",
            "inputSchema": { "type": "object", "properties": { "files": files }, "required": ["files"] },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "list_catalog",
            "description": "Every operation id, grouped by file family (image, video, audio, pdf…), without needing files.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "run_operation",
            "description": "Run one operation (e.g. convert:jpg, tool:compress, tool:fit-size, tool:merge) on the files. Returns the paths of the files produced. The originals are never modified.",
            "inputSchema": with(json!({
                "operation": { "type": "string", "description": "Operation id from list_operations." },
                "files": files,
            }), &["operation", "files"]),
            "annotations": { "destructiveHint": false },
        },
        {
            "name": "run_recipe",
            "description": "Run several operations in a row, each on the previous step's results (e.g. [\"tool:resize-1080\", \"convert:jpg\"]). Only the last step's files are kept.",
            "inputSchema": with(json!({
                "steps": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "Operation ids, in order." },
                "files": files,
            }), &["steps", "files"]),
            "annotations": { "destructiveHint": false },
        },
    ])
}

fn call(name: &str, args: &Value) -> Result<String> {
    match name {
        "list_catalog" => {
            let groups: Vec<Value> = catalog()
                .into_iter()
                .map(|(family, ops)| json!({ "family": family, "operations": ops }))
                .collect();
            Ok(serde_json::to_string_pretty(&groups)?)
        }
        "list_operations" => {
            let files = files(args)?;
            let ops: Vec<_> = [Category::Convert, Category::Tool, Category::Action]
                .into_iter()
                .flat_map(|c| operations(&files, c))
                // AirDrop, Float and Move… need the app's windows.
                .filter(|o| {
                    !matches!(
                        o.id.as_str(),
                        "action:airdrop" | "action:float" | "action:move"
                    )
                })
                .collect();
            Ok(serde_json::to_string_pretty(&ops)?)
        }
        "run_operation" | "run_recipe" => {
            let files = files(args)?;
            let opts = options(args)?;
            let outputs = if name == "run_operation" {
                let op = args["operation"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("`operation` is required"))?;
                run(op, &files, &opts, &|_| {})?
            } else {
                let steps: Vec<String> = serde_json::from_value(args["steps"].clone())
                    .map_err(|_| anyhow::anyhow!("`steps` must be a list of operation ids"))?;
                run_recipe(&steps, &files, &opts, &|_| {})?
            };
            Ok(serde_json::to_string_pretty(
                &json!({ "outputs": outputs }),
            )?)
        }
        other => anyhow::bail!("unknown tool `{other}`"),
    }
}

fn files(args: &Value) -> Result<Vec<PathBuf>> {
    let files: Vec<PathBuf> = serde_json::from_value(args["files"].clone())
        .map_err(|_| anyhow::anyhow!("`files` must be a list of paths"))?;
    if files.is_empty() {
        anyhow::bail!("no input files");
    }
    for f in &files {
        if !f.exists() {
            anyhow::bail!("{} does not exist", f.display());
        }
    }
    Ok(files)
}

fn options(args: &Value) -> Result<Options> {
    let mut opts = Options::default();
    if let Some(dir) = args["out_dir"].as_str() {
        std::fs::create_dir_all(dir)?;
        opts.out_dir = Some(dir.into());
    }
    if let Some(q) = args["quality"].as_u64() {
        opts.quality = q.clamp(1, 100) as u8;
    }
    if let Some(p) = args["video_preset"].as_str() {
        opts.video_preset = serde_json::from_value(json!(p))
            .map_err(|_| anyhow::anyhow!("video_preset must be standard, web or max"))?;
    }
    if let Some(mb) = args["target_mb"].as_f64() {
        opts.target_mb = mb.clamp(0.1, 10_000.0);
    }
    Ok(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, params: Value) -> Value {
        handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).unwrap()
    }

    #[test]
    fn initializes_and_lists_tools() {
        let init = request("initialize", json!({ "protocolVersion": "2025-03-26" }));
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        let tools = request("tools/list", json!({}));
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "list_operations",
                "list_catalog",
                "run_operation",
                "run_recipe"
            ]
        );
        // Notifications get no answer.
        assert!(
            handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none()
        );
    }

    #[test]
    fn runs_a_recipe_through_tools_call() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.png");
        image::RgbImage::from_pixel(2000, 1000, image::Rgb([10, 20, 30]))
            .save(&src)
            .unwrap();
        let reply = request(
            "tools/call",
            json!({ "name": "run_recipe", "arguments": {
                "steps": ["tool:resize-1080", "convert:jpg"],
                "files": [src],
            }}),
        );
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(reply["result"]["isError"].is_null(), "{text}");
        assert!(dir.path().join("a-1080.jpg").exists());

        let missing = request(
            "tools/call",
            json!({ "name": "run_operation", "arguments": { "operation": "convert:jpg", "files": ["/nope.png"] }}),
        );
        assert_eq!(missing["result"]["isError"], true);
    }
}
