//! `cargo xtask budgets`: the constitution VI / IV gates (research R16, R17).
//!
//! - cold start to the first `tools/list` response: median of 20 runs < 50 ms;
//! - stripped release binary < 10 MB;
//! - tool descriptions ≤ 3,000 tokens and the full `tools/list` ≤ 9,000 tokens, estimated as
//!   UTF-8 bytes ÷ 3.5;
//! - the default toolset has ≤ 25 tools.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const RUNS: usize = 20;
const START_BUDGET: Duration = Duration::from_millis(50);
const SIZE_BUDGET: u64 = 10 * 1024 * 1024;
const DESC_TOKENS: f64 = 3_000.0;
const LIST_TOKENS: f64 = 9_000.0;
const MAX_TOOLS: usize = 25;

fn tokens(bytes: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)] // byte counts are far below 2^52
    let b = bytes as f64;
    b / 3.5
}

fn binary(args: &[String]) -> PathBuf {
    args.windows(2)
        .find(|w| w[0] == "--bin")
        .map(|w| PathBuf::from(&w[1]))
        .or_else(|| std::env::var_os("GUILDSMITH_BIN").map(PathBuf::from))
        .unwrap_or_else(|| {
            let exe = if cfg!(windows) { "guildsmith.exe" } else { "guildsmith" };
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/release").join(exe)
        })
}

/// Starts `guildsmith mcp`, sends the handshake and `tools/list`, and returns the time to the
/// `tools/list` response plus its result.
fn one_run(bin: &Path, toolsets: Option<&str>) -> Result<(Duration, Value)> {
    let mut cmd = Command::new(bin);
    cmd.arg("mcp")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(t) = toolsets {
        cmd.env("GUILDSMITH_TOOLSETS", t);
    }
    let start = Instant::now();
    let mut child = cmd.spawn().with_context(|| format!("spawn {}", bin.display()))?;
    let mut stdin = child.stdin.take().context("stdin")?;
    for msg in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "budgets", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ] {
        writeln!(stdin, "{msg}")?;
    }
    stdin.flush()?;
    let reader = BufReader::new(child.stdout.take().context("stdout")?);
    let mut result = None;
    for line in reader.lines() {
        let v: Value = serde_json::from_str(&line?)?;
        if v["id"] == 2 {
            result = Some(v["result"].clone());
            break;
        }
    }
    let elapsed = start.elapsed();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    Ok((elapsed, result.context("no tools/list response")?))
}

pub fn run(args: &[String]) -> Result<()> {
    let bin = binary(args);
    if !bin.exists() {
        bail!("{} not found; build it first with `cargo build --release -p guildsmith`", bin.display());
    }
    let mut failures = Vec::new();

    let mut times = Vec::with_capacity(RUNS);
    let mut list = Value::Null;
    for _ in 0..RUNS {
        let (t, l) = one_run(&bin, None)?;
        times.push(t);
        list = l;
    }
    times.sort();
    let median = times[RUNS / 2];
    println!("cold start to tools/list: median {median:?} over {RUNS} runs (budget < {START_BUDGET:?})");
    if median >= START_BUDGET {
        failures.push(format!("cold start {median:?} ≥ {START_BUDGET:?}"));
    }

    let size = std::fs::metadata(&bin)?.len();
    #[allow(clippy::cast_precision_loss)] // binary sizes are far below 2^52
    let mib = size as f64 / 1_048_576.0;
    println!("binary size: {mib:.2} MiB (budget < 10 MiB)");
    if size >= SIZE_BUDGET {
        failures.push(format!("binary is {size} bytes ≥ {SIZE_BUDGET}"));
    }

    for (label, toolsets, check_count) in [("default", None, true), ("core,moderation", Some("core,moderation"), true)]
    {
        let list = if toolsets.is_some() { one_run(&bin, toolsets)?.1 } else { list.clone() };
        let tools = list["tools"].as_array().cloned().unwrap_or_default();
        let desc_bytes: usize = tools.iter().map(|t| t["description"].as_str().map_or(0, str::len)).sum();
        let list_bytes = serde_json::to_vec(&list)?.len();
        let (d, l) = (tokens(desc_bytes), tokens(list_bytes));
        println!("{label}: {} tools, descriptions ≈ {d:.0} tokens, tools/list ≈ {l:.0} tokens", tools.len());
        if check_count && tools.len() > MAX_TOOLS {
            failures.push(format!("{label}: {} tools > {MAX_TOOLS}", tools.len()));
        }
        if d > DESC_TOKENS {
            failures.push(format!("{label}: descriptions ≈ {d:.0} tokens > {DESC_TOKENS}"));
        }
        if l > LIST_TOKENS {
            failures.push(format!("{label}: tools/list ≈ {l:.0} tokens > {LIST_TOKENS}"));
        }
    }

    if failures.is_empty() {
        println!("all budgets met");
        Ok(())
    } else {
        bail!("budgets exceeded:\n  {}", failures.join("\n  "))
    }
}
