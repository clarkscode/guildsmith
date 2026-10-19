//! `cargo xtask check-plugins`: the agent-plugin checks CI runs (contracts/plugins.md).
//!
//! - both `plugin.json` files and both marketplace files parse, and their sources exist;
//! - plugin versions equal the workspace version;
//! - both `.mcp.json` files run the same pinned `npx -y guildsmith@<version> mcp`;
//! - `userConfig.discord_token` is a required, sensitive string; the token is forwarded, never
//!   written into a file;
//! - every skill has `name` (matching its directory) and `description` frontmatter;
//! - `plugins/*/skills/` match `plugins/skills/` byte for byte.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::sync;

fn read_json(root: &Path, rel: &str) -> Result<Value> {
    let path = root.join(rel);
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {rel}"))?;
    serde_json::from_str(&text).with_context(|| format!("{rel} is not valid JSON"))
}

/// The `key: value` lines of a SKILL.md YAML frontmatter block.
fn frontmatter(text: &str) -> Option<Vec<(&str, &str)>> {
    let body = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n"))?;
    let end = body.find("\n---")?;
    Some(
        body[..end]
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim(), v.trim().trim_matches('"')))
            .collect(),
    )
}

struct Checks {
    problems: Vec<String>,
}

impl Checks {
    fn expect(&mut self, ok: bool, problem: impl FnOnce() -> String) {
        if !ok {
            self.problems.push(problem());
        }
    }
}

fn check_manifests(c: &mut Checks, root: &Path, version: &str) -> Result<()> {
    for rel in ["plugins/claude-code/.claude-plugin/plugin.json", "plugins/codex/.codex-plugin/plugin.json"] {
        let m = read_json(root, rel)?;
        c.expect(m["name"] == "guildsmith", || format!("{rel}: name must be `guildsmith`"));
        c.expect(m["version"] == version, || format!("{rel}: version {} ≠ workspace version {version}", m["version"]));
    }

    let claude = read_json(root, "plugins/claude-code/.claude-plugin/plugin.json")?;
    let token = &claude["userConfig"]["discord_token"];
    c.expect(token["type"] == "string", || "userConfig.discord_token must be a string".to_owned());
    c.expect(token["sensitive"] == true, || "userConfig.discord_token must be \"sensitive\": true".to_owned());
    c.expect(token["required"] == true, || "userConfig.discord_token must be required".to_owned());
    c.expect(claude["userConfig"]["guilds"]["required"] != true, || "userConfig.guilds must be optional".to_owned());

    let pinned = format!("guildsmith@{version}");
    let expected_args = Value::from(vec!["-y", pinned.as_str(), "mcp"]);
    for rel in ["plugins/claude-code/.mcp.json", "plugins/codex/.mcp.json"] {
        let server = &read_json(root, rel)?["mcpServers"]["guildsmith"];
        c.expect(server["command"] == "npx", || format!("{rel}: command must be `npx`"));
        c.expect(server["args"] == expected_args, || {
            format!("{rel}: args must be {expected_args}, got {}", server["args"])
        });
    }
    let claude_env = &read_json(root, "plugins/claude-code/.mcp.json")?["mcpServers"]["guildsmith"]["env"];
    c.expect(claude_env["DISCORD_TOKEN"] == "${user_config.discord_token}", || {
        "plugins/claude-code/.mcp.json: DISCORD_TOKEN must come from ${user_config.discord_token}".to_owned()
    });
    c.expect(claude_env["GUILDSMITH_GUILDS"] == "${user_config.guilds}", || {
        "plugins/claude-code/.mcp.json: GUILDSMITH_GUILDS must come from ${user_config.guilds}".to_owned()
    });
    let codex_vars = &read_json(root, "plugins/codex/.mcp.json")?["mcpServers"]["guildsmith"]["env_vars"];
    c.expect(*codex_vars == Value::from(vec!["DISCORD_TOKEN", "GUILDSMITH_GUILDS"]), || {
        "plugins/codex/.mcp.json: env_vars must be [\"DISCORD_TOKEN\", \"GUILDSMITH_GUILDS\"]".to_owned()
    });
    Ok(())
}

fn check_marketplaces(c: &mut Checks, root: &Path) -> Result<()> {
    let claude = read_json(root, ".claude-plugin/marketplace.json")?;
    let source = claude["plugins"][0]["source"].as_str().unwrap_or_default();
    c.expect(source == "./plugins/claude-code", || {
        format!(".claude-plugin/marketplace.json: source must be ./plugins/claude-code, got `{source}`")
    });
    let codex = read_json(root, ".agents/plugins/marketplace.json")?;
    let path = codex["plugins"][0]["source"]["path"].as_str().unwrap_or_default();
    c.expect(path == "./plugins/codex", || {
        format!(".agents/plugins/marketplace.json: source.path must be ./plugins/codex, got `{path}`")
    });
    Ok(())
}

fn check_skills(c: &mut Checks, root: &Path) -> Result<()> {
    let source = sync::files(&root.join(sync::SOURCE))?;
    c.expect(!source.is_empty(), || format!("{}/ has no skills", sync::SOURCE));
    for (rel, bytes) in &source {
        let Some(dir) = rel.strip_suffix("/SKILL.md") else { continue };
        let text = String::from_utf8_lossy(bytes);
        let Some(fields) = frontmatter(&text) else {
            c.problems.push(format!("{}/{rel}: missing YAML frontmatter", sync::SOURCE));
            continue;
        };
        let get = |k: &str| fields.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
        c.expect(get("name") == Some(dir), || format!("{}/{rel}: frontmatter name must be `{dir}`", sync::SOURCE));
        c.expect(get("description").is_some_and(|d| !d.is_empty()), || {
            format!("{}/{rel}: frontmatter needs a description", sync::SOURCE)
        });
    }
    for copy in sync::COPIES {
        let copied = sync::files(&root.join(copy))?;
        for (rel, bytes) in &source {
            match copied.get(rel) {
                None => c.problems.push(format!("{copy}/{rel} is missing")),
                Some(b) if b != bytes => c.problems.push(format!("{copy}/{rel} differs from {}/{rel}", sync::SOURCE)),
                Some(_) => {}
            }
        }
        for rel in copied.keys().filter(|k| !source.contains_key(*k)) {
            c.problems.push(format!("{copy}/{rel} is not in {}/", sync::SOURCE));
        }
    }
    Ok(())
}

pub fn run(_args: &[String]) -> Result<()> {
    let root = crate::root();
    let version = crate::workspace_version()?;
    let mut c = Checks { problems: Vec::new() };
    check_manifests(&mut c, &root, &version)?;
    check_marketplaces(&mut c, &root)?;
    check_skills(&mut c, &root)?;
    if c.problems.is_empty() {
        println!("plugins: manifests valid, pinned to guildsmith@{version}, skills in sync");
        Ok(())
    } else {
        bail!("plugin checks failed (run `cargo xtask sync-plugins` for skill drift):\n  {}", c.problems.join("\n  "))
    }
}

#[cfg(test)]
mod tests {
    use super::frontmatter;

    #[test]
    fn reads_frontmatter_fields() {
        let text = "---\nname: guild-plan\ndescription: \"Plan a file\"\nuser-invocable: true\n---\n# Body\n";
        let fields = frontmatter(text).unwrap();
        assert!(fields.contains(&("name", "guild-plan")));
        assert!(fields.contains(&("description", "Plan a file")));
        assert!(frontmatter("# no frontmatter").is_none());
    }
}
