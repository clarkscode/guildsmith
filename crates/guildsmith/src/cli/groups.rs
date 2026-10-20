//! `guildsmith <group> <tool> [--flags]` (contracts/cli.md, FR-008): every tool from the same
//! registry as the MCP server, run in-process through the same handler, so the CLI and agents
//! share services and policy.
//!
//! - Flags come from the tool's input schema: `--name value` (`--name=value`), `--flag` /
//!   `--no-flag` for booleans, comma lists or repeated flags for arrays, JSON for objects.
//!   Required inputs may also be given in order as positional arguments. `--args '{json}'`
//!   passes any input as JSON.
//! - `--yes` stands in for `confirm: true`. Without it, a terminal shows the preview and asks;
//!   anything else prints the preview and exits 3.
//! - `guildsmith raw <METHOD> <path>` replaces `{guild}` in the path with the `-g` server id.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use guildsmith_app::App;
use guildsmith_app::ports::DiscordPort;
use guildsmith_app::toolsets::Toolset;
use guildsmith_mcp::GuildsmithServer;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig, JsonObject, ProgressNotificationParam, Tool};
use rmcp::service::{NotificationContext, RunningService};
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::{Map, Value};

use super::Exit;
use super::apply::Confirm;
use crate::render;

/// Prints bulk progress to stderr.
#[derive(Debug, Clone, Default)]
struct CliClient;

impl ClientHandler for CliClient {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }

    async fn on_progress(&self, params: ProgressNotificationParam, _context: NotificationContext<RoleClient>) {
        let msg = params.message.unwrap_or_default();
        eprintln!("progress: {msg}");
    }
}

/// Options this module reads itself, from anywhere after the group name.
#[derive(Debug, Default, PartialEq, Eq)]
struct Globals {
    guild: Option<String>,
    json: bool,
    yes: bool,
}

/// A tool's short CLI names, most preferred first: without its first word (`member_ban` →
/// `ban`), then without its last word (`template_import` → `template`).
fn aliases(tool: &str) -> [Option<String>; 2] {
    [
        tool.split_once('_').map(|(_, rest)| rest.replace('_', "-")),
        tool.rsplit_once('_').map(|(first, _)| first.replace('_', "-")),
    ]
}

/// Finds the tool `name` names in `group` (or, failing that, in any group when unique).
fn resolve_tool(group: Toolset, name: &str) -> Result<&'static str, String> {
    let wanted = name.replace('_', "-");
    let find = |tools: &[&'static str]| -> Vec<&'static str> {
        // An exact name wins, then the first kind of short name, then the second.
        if let Some(t) = tools.iter().find(|t| t.replace('_', "-") == wanted) {
            return vec![*t];
        }
        for tier in 0..2 {
            let hits: Vec<&'static str> =
                tools.iter().copied().filter(|t| aliases(t)[tier].as_deref() == Some(wanted.as_str())).collect();
            if !hits.is_empty() {
                return hits;
            }
        }
        Vec::new()
    };
    let mut hits = find(group.tools());
    if hits.is_empty() {
        let all: Vec<&'static str> = Toolset::ALL.iter().flat_map(|t| t.tools()).copied().collect();
        hits = find(&all);
    }
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "no tool `{name}` in the {group} group; its tools are: {}",
            group.tools().iter().map(|t| t.replace('_', "-")).collect::<Vec<_>>().join(", ")
        )),
        many => Err(format!("`{name}` could mean {}; use the full tool name", many.join(", "))),
    }
}

/// Follows `$ref` and unwraps `[T, "null"]` and `anyOf` with null, so `kind()` sees the type.
fn resolve<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str)
        && let Some(name) = r.strip_prefix("#/$defs/").or_else(|| r.strip_prefix("#/definitions/"))
        && let Some(def) = root.get("$defs").or_else(|| root.get("definitions")).and_then(|d| d.get(name))
    {
        return resolve(root, def);
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(alts)) = schema.get(key) {
            let non_null: Vec<&Value> = alts.iter().filter(|a| a.get("type") != Some(&Value::from("null"))).collect();
            if let [one] = non_null.as_slice() {
                return resolve(root, one);
            }
        }
    }
    schema
}

/// A schema's main type, ignoring `null`.
fn kind(schema: &Value) -> &str {
    match schema.get("type") {
        Some(Value::String(t)) => t.as_str(),
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).find(|t| *t != "null").unwrap_or("string"),
        _ if schema.get("enum").is_some() => "string",
        _ => "any",
    }
}

/// `90`, `90s`, `15m`, `2h`, `1d`, `1w` in the unit the input's description names (seconds by
/// default).
fn parse_number(value: &str, description: &str) -> Option<Value> {
    if let Ok(n) = value.parse::<i64>() {
        return Some(n.into());
    }
    if let Ok(f) = value.parse::<f64>() {
        return serde_json::Number::from_f64(f).map(Value::Number);
    }
    let split = value.find(|c: char| !c.is_ascii_digit())?;
    let (n, unit) = value.split_at(split);
    let n: u64 = n.parse().ok()?;
    let secs = n * match unit {
        "s" => 1,
        "m" | "min" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        _ => return None,
    };
    // The unit the description names first: "Seconds, at most 28 days" is in seconds.
    let d = description.to_ascii_lowercase();
    let per = [("second", 1), ("minute", 60), ("hour", 3_600), ("day", 86_400)]
        .iter()
        .filter_map(|(word, per)| d.find(word).map(|at| (at, *per)))
        .min()
        .map_or(1, |(_, per)| per);
    secs.is_multiple_of(per).then(|| Value::from(secs / per))
}

/// `a=1&b=2` (or `a=1,b=2`) as an object of strings; a repeated key becomes a list.
fn pairs(raw: &str) -> Value {
    let mut obj = JsonObject::new();
    for (k, v) in raw.split(['&', ',']).filter_map(|p| p.split_once('=')) {
        add(&mut obj, k.trim().to_owned(), Value::String(v.trim().to_owned()));
    }
    Value::Object(obj)
}

/// Adds `v` under `k`, turning an existing value into a list.
fn add(obj: &mut JsonObject, k: String, v: Value) {
    match obj.get_mut(&k) {
        Some(Value::Array(have)) => have.push(v),
        Some(have) => *have = Value::Array(vec![have.take(), v]),
        None => {
            obj.insert(k, v);
        }
    }
}

/// Turns one flag value into JSON for an input of `schema`.
fn value_for(root: &Value, schema: &Value, raw: &str, flag: &str) -> Result<Value, String> {
    let s = resolve(root, schema);
    let desc = s.get("description").or_else(|| schema.get("description")).and_then(Value::as_str).unwrap_or("");
    let bad = |what: &str| format!("--{flag}: expected {what}, got `{raw}`");
    match kind(s) {
        "boolean" => match raw {
            "true" | "yes" | "1" => Ok(Value::Bool(true)),
            "false" | "no" | "0" => Ok(Value::Bool(false)),
            _ => Err(bad("true or false")),
        },
        "integer" | "number" => parse_number(raw, desc).ok_or_else(|| bad("a number (or 30s, 15m, 2h, 1d)")),
        "array" => {
            if raw.trim_start().starts_with('[') {
                return serde_json::from_str(raw).map_err(|e| format!("--{flag}: {e}"));
            }
            let items = s.get("items").unwrap_or(&Value::Null);
            raw.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(|p| value_for(root, items, p, flag))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        "object" if !raw.trim_start().starts_with('{') && raw.contains('=') => Ok(pairs(raw)),
        "object" => serde_json::from_str(raw).map_err(|e| format!("--{flag}: expected JSON: {e}")),
        "string" => Ok(Value::String(raw.to_owned())),
        _ => Ok(serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()))),
    }
}

/// Builds the tool's arguments from command-line words.
fn parse_args(tool: &Tool, words: &[String]) -> Result<(JsonObject, Globals), String> {
    let root = Value::Object(tool.input_schema.as_ref().clone());
    let props = root.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
    let required: Vec<String> = root
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let mut args = Map::new();
    let mut globals = Globals::default();
    let mut positionals = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = &words[i];
        i += 1;
        let Some(flag) = w.strip_prefix("--").or_else(|| (w.len() == 2 && w.starts_with('-')).then(|| &w[1..])) else {
            positionals.push(w.clone());
            continue;
        };
        let (name, inline) = match flag.split_once('=') {
            Some((n, v)) => (n.to_owned(), Some(v.to_owned())),
            None => (flag.to_owned(), None),
        };
        let mut take = |what: &str| -> Result<String, String> {
            if let Some(v) = &inline {
                return Ok(v.clone());
            }
            let v = words.get(i).cloned().ok_or_else(|| format!("--{what} needs a value"))?;
            i += 1;
            Ok(v)
        };
        match name.as_str() {
            "g" | "guild" => {
                globals.guild = Some(take("guild")?);
                continue;
            }
            "json" => {
                globals.json = true;
                continue;
            }
            "y" | "yes" => {
                globals.yes = true;
                continue;
            }
            "read-only" => continue, // already applied when the config was loaded
            "args" => {
                let v: Value = serde_json::from_str(&take("args")?).map_err(|e| format!("--args: {e}"))?;
                let Value::Object(obj) = v else { return Err("--args must be a JSON object".to_owned()) };
                args.extend(obj);
                continue;
            }
            _ => {}
        }
        let key = name.replace('-', "_");
        if let Some(schema) = props.get(&key) {
            let is_bool = kind(resolve(&root, schema)) == "boolean";
            let raw = if is_bool && inline.is_none() { "true".to_owned() } else { take(&name)? };
            let v = value_for(&root, schema, &raw, &name)?;
            match (args.get_mut(&key), v) {
                (Some(Value::Array(have)), Value::Array(more)) => have.extend(more),
                (Some(Value::Object(have)), Value::Object(more)) => {
                    for (k, v) in more {
                        add(have, k, v);
                    }
                }
                (_, v) => {
                    args.insert(key, v);
                }
            }
        } else if let Some(neg) = key.strip_prefix("no_").filter(|k| props.contains_key(*k)) {
            args.insert(neg.to_owned(), Value::Bool(false));
        } else {
            let mut known: Vec<String> = props.keys().map(|k| format!("--{}", k.replace('_', "-"))).collect();
            known.sort();
            return Err(format!("unknown option --{name} for {}; options: {}", tool.name, known.join(", ")));
        }
    }
    // Positionals fill the required inputs in order, then a target list (`members`, `users`).
    let open: Vec<String> = required.iter().filter(|r| *r != "guild" && !args.contains_key(*r)).cloned().collect();
    let mut open = open.iter();
    let list = ["members", "users"].into_iter().find(|k| props.contains_key(*k));
    for p in positionals {
        if let Some(key) = open.next() {
            let v = value_for(&root, &props[key], &p, key)?;
            args.insert(key.clone(), v);
        } else if let Some(key) = list {
            let Value::Array(more) = value_for(&root, &props[key], &p, key)? else { unreachable!("a list") };
            match args.entry(key.to_owned()).or_insert_with(|| Value::Array(Vec::new())) {
                Value::Array(have) => have.extend(more),
                other => *other = Value::Array(more),
            }
        } else {
            return Err(format!("unexpected argument `{p}`"));
        }
    }
    Ok((args, globals))
}

async fn connect<P: DiscordPort + 'static>(
    server: GuildsmithServer<P>,
) -> Option<RunningService<RoleClient, CliClient>> {
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    CliClient.serve(client_io).await.ok()
}

fn texts(r: &CallToolResult) -> Vec<String> {
    r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect()
}

/// Prints a result: the summary (and, for people, the details), or the structured JSON.
fn print(r: &CallToolResult, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> Exit {
    let sc = r.structured_content.clone().unwrap_or(Value::Null);
    if r.is_error == Some(true) {
        if let Some(refusal) = sc.get("refusal") {
            let _ = writeln!(err, "refused: {}", refusal["reason"].as_str().unwrap_or_default());
            if let Some(h) = refusal["hint"].as_str() {
                let _ = writeln!(err, "hint: {h}");
            }
            return Exit::Refused;
        }
        let _ = writeln!(err, "error: {}", sc["error"].as_str().map_or_else(|| texts(r).join("\n"), str::to_owned));
        return Exit::Error;
    }
    if json {
        render::json(out, &sc);
    } else {
        let t = texts(r);
        let _ = writeln!(out, "{}", t.first().map_or("", String::as_str));
        if let Some(details) = t.get(1) {
            // Untrusted content stays in its wrapper; other results are shown as pretty JSON.
            let shown = serde_json::from_str::<Value>(details)
                .ok()
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or_else(|| details.clone());
            let _ = writeln!(out, "{shown}");
        }
    }
    Exit::Ok
}

fn is_preview(r: &CallToolResult) -> bool {
    r.is_error != Some(true) && r.structured_content.as_ref().is_some_and(|v| v["preview"] == true)
}

/// The tool's arguments: flags, the server (`-g`, which also fills `{guild}` in raw paths), and `--yes`.
fn prepare(def: &Tool, words: &[String], cli_guild: Option<&str>) -> Result<(JsonObject, Globals), String> {
    let (mut args, globals) = parse_args(def, words)?;
    let guild = globals.guild.as_deref().or(cli_guild);
    let has = |k: &str| def.input_schema.get("properties").and_then(|p| p.get(k)).is_some();
    if let Some(g) = guild
        && has("guild")
        && !args.contains_key("guild")
    {
        args.insert("guild".to_owned(), Value::String(g.to_owned()));
    }
    if globals.yes && has("confirm") {
        args.insert("confirm".to_owned(), Value::Bool(true));
    }
    Ok((args, globals))
}

/// Runs `guildsmith <group> <tool> …`. `words[0]` is the group name.
pub async fn run<P: DiscordPort + 'static>(
    app: Arc<App<P>>,
    cli_guild: Option<&str>,
    cli_json: bool,
    words: &[String],
    io: (&mut dyn Write, &mut dyn Write),
    confirm: &mut Confirm<'_>,
) -> Exit {
    let (out, err) = io;
    let fail = |err: &mut dyn Write, msg: &str| {
        let _ = writeln!(err, "error: {msg}");
        Exit::Error
    };
    let Some(group_name) = words.first() else { return fail(err, "missing command") };
    let group: Toolset = match group_name.parse() {
        Ok(g) => g,
        Err(_) => return fail(err, &format!("unknown command `{group_name}`; see `guildsmith --help`")),
    };
    let mut rest: Vec<String> = words[1..].to_vec();
    let tool_name = if group == Toolset::Raw {
        // `raw <METHOD> <path>`: both positional.
        "discord_request"
    } else {
        if rest.is_empty() || rest[0].starts_with('-') {
            let list: Vec<String> = group.tools().iter().map(|t| t.replace('_', "-")).collect();
            return fail(err, &format!("which {group} tool? one of: {}", list.join(", ")));
        }
        match resolve_tool(group, &rest.remove(0)) {
            Ok(t) => t,
            Err(e) => return fail(err, &e),
        }
    };
    let stop = Arc::new(AtomicBool::new(false));
    let server = GuildsmithServer::for_cli(app.clone(), stop.clone());
    let Some(def) = server.tool_definition(tool_name) else {
        return fail(err, &format!("{tool_name} isn't available in this build yet"));
    };
    let (mut args, globals) = match prepare(&def, &rest, cli_guild) {
        Ok(v) => v,
        Err(e) => return fail(err, &e),
    };
    let json = cli_json || globals.json;
    let confirmable = def.input_schema.get("properties").and_then(|p| p.get("confirm")).is_some();

    let Some(client) = connect(server).await else { return fail(err, "couldn't start the tool server") };
    let stop_on_ctrl_c = {
        let stop = stop.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("stopping after the current item…");
                stop.store(true, Ordering::Relaxed);
            }
        })
    };
    let call = |args: JsonObject| {
        let mut params = CallToolRequestParams::new(tool_name);
        params.arguments = Some(args);
        client.call_tool(params)
    };
    let result = match call(args.clone()).await {
        Ok(r) => r,
        Err(e) => {
            stop_on_ctrl_c.abort();
            return fail(err, &e.to_string());
        }
    };
    let exit = if confirmable && is_preview(&result) && !globals.yes {
        // Show the preview on stderr, then ask (or refuse without a terminal).
        let mut shown = Vec::new();
        let _ = print(&result, false, &mut shown, err);
        let _ = err.write_all(&shown);
        match confirm("Go ahead? [y/N] ") {
            Some(true) => {
                args.insert("confirm".to_owned(), Value::Bool(true));
                match call(args).await {
                    Ok(r) => print(&r, json, out, err),
                    Err(e) => fail(err, &e.to_string()),
                }
            }
            Some(false) => {
                let _ = writeln!(err, "Cancelled; nothing was changed.");
                Exit::Refused
            }
            None => {
                let _ = writeln!(err, "refused: not a terminal, so there is no one to confirm; nothing was changed");
                let _ = writeln!(err, "hint: review the preview, then run again with --yes");
                Exit::Refused
            }
        }
    } else {
        print(&result, json, out, err)
    };
    stop_on_ctrl_c.abort();
    let _ = client.cancel().await;
    exit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_resolve_within_the_group() {
        assert_eq!(resolve_tool(Toolset::Moderation, "ban"), Ok("member_ban"));
        assert_eq!(resolve_tool(Toolset::Moderation, "member-ban"), Ok("member_ban"));
        assert_eq!(resolve_tool(Toolset::Moderation, "purge"), Ok("messages_purge"));
        assert_eq!(resolve_tool(Toolset::Moderation, "prune"), Ok("members_prune"));
        assert_eq!(resolve_tool(Toolset::Settings, "template-import"), Ok("template_import"));
        assert_eq!(resolve_tool(Toolset::Events, "create"), Ok("event_create"));
        // Found in another group when unique there.
        assert_eq!(resolve_tool(Toolset::Messages, "purge"), Ok("messages_purge"));
        assert!(resolve_tool(Toolset::Polls, "nope").is_err());
    }

    #[test]
    fn numbers_take_units_from_the_description() {
        assert_eq!(parse_number("1d", "Seconds to delete"), Some(Value::from(86_400)));
        assert_eq!(parse_number("2d", "Duration in hours"), Some(Value::from(48)));
        assert_eq!(parse_number("1h", "Seconds, at most 28 days (2419200)"), Some(Value::from(3_600)));
        assert_eq!(parse_number("90", "anything"), Some(Value::from(90)));
        assert_eq!(parse_number("1x", "seconds"), None);
    }
}
