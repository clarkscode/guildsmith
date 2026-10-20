//! Deterministic spec file writer (research R11). Values are encoded by `toml_edit`, so quoting
//! and escaping follow TOML exactly; the layout (order, comments, inline overwrite tables) is
//! fixed so exporting twice gives byte-identical files.

use std::fmt::Write as _;

use serde_json::Value;

use crate::export::ExportNotes;
use crate::permissions::PermissionSet;
use crate::spec::{CategorySpec, ChannelSpec, ForumTag, GuildSpec, OverwriteSpec, RoleSpec};

pub const SCHEMA_URL: &str = "https://raw.githubusercontent.com/clarkscode/guildsmith/main/schema/guild.schema.json";

/// The provenance line at the top of an export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header<'a> {
    pub version: &'a str,
    pub guild_name: &'a str,
    pub guild_id: &'a str,
    /// `YYYY-MM-DD`.
    pub date: &'a str,
}

fn q(s: &str) -> String {
    toml_edit::Value::from(s).to_string().trim().to_owned()
}

fn perms(p: PermissionSet) -> String {
    let names: Vec<String> = p.names().iter().map(|n| q(n)).collect();
    format!("[{}]", names.join(", "))
}

fn overwrite_inline(o: &OverwriteSpec) -> String {
    let mut parts = vec![format!("target = {}", q(&o.target.to_string()))];
    if !o.allow.is_empty() {
        parts.push(format!("allow = {}", perms(o.allow)));
    }
    if !o.deny.is_empty() {
        parts.push(format!("deny = {}", perms(o.deny)));
    }
    format!("{{ {} }}", parts.join(", "))
}

fn tag_inline(t: &ForumTag) -> String {
    let mut parts = Vec::new();
    if let Some(id) = t.id {
        parts.push(format!("id = {}", q(&id.to_string())));
    }
    parts.push(format!("name = {}", q(&t.name)));
    if let Some(e) = &t.emoji {
        parts.push(format!("emoji = {}", q(e)));
    }
    if let Some(m) = t.moderated {
        parts.push(format!("moderated = {m}"));
    }
    format!("{{ {} }}", parts.join(", "))
}

fn write_overwrites(out: &mut String, indent: &str, list: &[OverwriteSpec]) {
    if list.is_empty() {
        let _ = writeln!(out, "{indent}overwrites = []");
        return;
    }
    let _ = writeln!(out, "{indent}overwrites = [");
    for o in list {
        let _ = writeln!(out, "{indent}  {},", overwrite_inline(o));
    }
    let _ = writeln!(out, "{indent}]");
}

fn write_role(out: &mut String, r: &RoleSpec, notes: &ExportNotes) {
    out.push_str("\n[[roles]]\n");
    if let Some(id) = r.id {
        let _ = writeln!(out, "id = {}", q(&id.to_string()));
    }
    let _ = writeln!(out, "name = {}", q(&r.name));
    if let Some(c) = r.color {
        let _ = writeln!(out, "color = {}", q(&c.to_string()));
    }
    if let Some((sec, ter)) = r.id.and_then(|id| notes.gradients.get(&id)) {
        let fmt = |c: &Option<u32>| c.map_or_else(|| "none".to_owned(), |c| format!("#{c:06X}"));
        let _ = writeln!(out, "# gradient colors (read-only): secondary {}, tertiary {}", fmt(sec), fmt(ter));
    }
    for (key, v) in [("hoist", r.hoist), ("mentionable", r.mentionable)] {
        if let Some(v) = v {
            let _ = writeln!(out, "{key} = {v}");
        }
    }
    if let Some(p) = r.permissions {
        let _ = writeln!(out, "permissions = {}", perms(p));
    }
}

fn write_channel(out: &mut String, header: &str, indent: &str, c: &ChannelSpec) {
    let _ = writeln!(out, "\n{indent}[[{header}]]");
    let line = |out: &mut String, k: &str, v: String| {
        let _ = writeln!(out, "{indent}{k} = {v}");
    };
    if let Some(id) = c.id {
        line(out, "id", q(&id.to_string()));
    }
    line(out, "name", q(&c.name));
    line(out, "kind", q(c.kind.as_str()));
    if let Some(t) = &c.topic {
        line(out, "topic", q(t));
    }
    if let Some(v) = c.nsfw {
        line(out, "nsfw", v.to_string());
    }
    if let Some(v) = c.slowmode {
        line(out, "slowmode", v.to_string());
    }
    if let Some(v) = c.bitrate {
        line(out, "bitrate", v.to_string());
    }
    if let Some(v) = c.user_limit {
        line(out, "user_limit", v.to_string());
    }
    if let Some(v) = &c.rtc_region {
        line(out, "rtc_region", q(v));
    }
    if let Some(tags) = &c.tags {
        let inner: Vec<String> = tags.iter().map(tag_inline).collect();
        line(out, "tags", format!("[{}]", inner.join(", ")));
    }
    if let Some(r) = &c.default_reaction {
        line(out, "default_reaction", q(r));
    }
    if c.sync_permissions {
        line(out, "sync_permissions", "true".to_owned());
    }
    if let Some(o) = &c.overwrites {
        write_overwrites(out, indent, o);
    }
}

fn write_category(out: &mut String, cat: &CategorySpec) {
    out.push_str("\n[[categories]]\n");
    if let Some(id) = cat.id {
        let _ = writeln!(out, "id = {}", q(&id.to_string()));
    }
    let _ = writeln!(out, "name = {}", q(&cat.name));
    if let Some(o) = &cat.overwrites {
        write_overwrites(out, "", o);
    }
    for ch in &cat.channels {
        write_channel(out, "categories.channels", "  ", ch);
    }
}

/// Formats an exported spec, with the schema header, provenance and read-only comments.
#[must_use]
pub fn write(spec: &GuildSpec, header: &Header<'_>, notes: &ExportNotes) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "#:schema {SCHEMA_URL}");
    let _ = writeln!(
        out,
        "# Exported by guildsmith {} from {} ({}) on {}.",
        header.version,
        q(header.guild_name),
        header.guild_id,
        header.date
    );
    if notes.incomplete {
        out.push_str(
            "# WARNING: the bot is not an administrator, so channels it can't view are not listed.\n\
             # Planning this file against the server needs Administrator (docs/adr/001).\n",
        );
    }

    body(&mut out, spec, notes);
    out
}

/// Where an imported template came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateHeader<'a> {
    pub version: &'a str,
    pub code: &'a str,
    pub template_name: &'a str,
    /// `YYYY-MM-DD`.
    pub date: &'a str,
}

/// Formats a spec imported from a template (no ids: items are matched by name).
#[must_use]
pub fn write_template(spec: &GuildSpec, header: &TemplateHeader<'_>, notes: &ExportNotes) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "#:schema {SCHEMA_URL}");
    let _ = writeln!(
        out,
        "# Imported by guildsmith {} from the template {} ({}) on {}.",
        header.version,
        q(header.template_name),
        header.code,
        header.date
    );
    out.push_str(
        "# Template ids are placeholders, so this file has none: items are matched by name when it is\n\
         # planned. guildsmith never creates servers (bots can't); plan this against a server you made.\n",
    );
    body(&mut out, spec, notes);
    out
}

/// A TOML key: bare when it can be, quoted otherwise.
fn key(k: &str) -> String {
    if !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        k.to_owned()
    } else {
        q(k)
    }
}

/// Fields in writing order: identity first (`id`, `code`, `name`, `title`, `channel`), then
/// the rest alphabetically.
fn ordered(m: &serde_json::Map<String, Value>) -> Vec<(&String, &Value)> {
    const FIRST: [&str; 5] = ["id", "code", "name", "title", "channel"];
    let mut v: Vec<(&String, &Value)> = m.iter().filter(|(_, x)| !x.is_null()).collect();
    v.sort_by_key(|(k, _)| (FIRST.iter().position(|f| f == k).unwrap_or(FIRST.len()), (*k).clone()));
    v
}

/// A JSON value as an inline TOML value (`null`s are left out of tables).
fn inline(v: &Value) -> String {
    match v {
        Value::Null => "\"\"".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => q(s),
        Value::Array(a) => format!("[{}]", a.iter().map(inline).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => {
            let parts: Vec<String> =
                ordered(m).into_iter().map(|(k, v)| format!("{} = {}", key(k), inline(v))).collect();
            if parts.is_empty() { "{}".to_owned() } else { format!("{{ {} }}", parts.join(", ")) }
        }
    }
}

/// `key = value` lines for a table's fields. `start` and `end` are written as TOML datetimes,
/// long lists one item per line, and `skip` keys are left for the caller.
fn fields(out: &mut String, indent: &str, v: &Value, skip: &[&str]) {
    let Value::Object(m) = v else { return };
    for (k, x) in ordered(m).into_iter().filter(|(k, _)| !skip.contains(&k.as_str())) {
        let text = match (k.as_str(), x) {
            ("start" | "end", Value::String(t)) => t.clone(),
            (_, Value::Array(a)) if a.iter().any(Value::is_object) && a.len() > 1 => {
                let items: Vec<String> = a.iter().map(|i| format!("{indent}  {},", inline(i))).collect();
                format!("[\n{}\n{indent}]", items.join("\n"))
            }
            _ => inline(x),
        };
        let _ = writeln!(out, "{indent}{} = {text}", key(k));
    }
}

fn sections<T: serde::Serialize>(out: &mut String, header: &str, items: &[T]) {
    for item in items {
        let _ = writeln!(out, "\n[[{header}]]");
        fields(out, "", &serde_json::to_value(item).unwrap_or(Value::Null), &[]);
    }
}

/// The sections beyond roles and channels (US9), in the spec's order.
fn write_kinds(out: &mut String, spec: &GuildSpec) {
    sections(out, "automod", &spec.automod);
    if let Some(o) = &spec.onboarding {
        let v = serde_json::to_value(o).unwrap_or(Value::Null);
        out.push_str("\n[onboarding]\n");
        fields(out, "", &v, &["prompts"]);
        for p in v["prompts"].as_array().into_iter().flatten() {
            out.push_str("\n  [[onboarding.prompts]]\n");
            fields(out, "  ", p, &[]);
        }
    }
    if let Some(w) = &spec.welcome_screen {
        out.push_str("\n[welcome_screen]\n");
        fields(out, "", &serde_json::to_value(w).unwrap_or(Value::Null), &[]);
    }
    sections(out, "emoji", &spec.emoji);
    sections(out, "stickers", &spec.stickers);
    sections(out, "sounds", &spec.sounds);
    sections(out, "webhooks", &spec.webhooks);
    sections(out, "invites", &spec.invites);
    sections(out, "events", &spec.events);
    sections(out, "messages", &spec.messages);
}

/// Whether the spec has anything beyond roles and channels.
fn has_kinds(spec: &GuildSpec) -> bool {
    spec.guild.manage_settings
        || !spec.automod.is_empty()
        || spec.onboarding.is_some()
        || spec.welcome_screen.is_some()
        || !spec.emoji.is_empty()
        || !spec.stickers.is_empty()
        || !spec.sounds.is_empty()
        || !spec.webhooks.is_empty()
        || !spec.invites.is_empty()
        || !spec.events.is_empty()
        || !spec.messages.is_empty()
}

fn body(out: &mut String, spec: &GuildSpec, notes: &ExportNotes) {
    out.push_str("\n[guild]\n");
    if let Some(id) = spec.guild.id {
        let _ = writeln!(out, "id = {}", q(&id.to_string()));
    }
    if let Some(n) = &spec.guild.name {
        let _ = writeln!(out, "name = {}  # informational", q(n));
    }
    if spec.guild.manage_settings {
        out.push_str("manage_settings = true\n");
    }
    if let Some(st) = &spec.guild.settings {
        out.push_str("\n[guild.settings]\n");
        fields(out, "", &serde_json::to_value(st).unwrap_or(Value::Null), &[]);
    }

    if !spec.roles.is_empty() {
        out.push_str("\n# Roles are listed highest first.");
    }
    for r in &spec.roles {
        write_role(out, r, notes);
    }
    for m in &notes.managed_roles {
        let _ = writeln!(out, "\n# managed (read-only): {m}");
    }
    for cat in &spec.categories {
        write_category(out, cat);
    }
    for ch in &spec.channels {
        write_channel(out, "channels", "", ch);
    }
    write_kinds(out, spec);
    for u in &notes.unmanaged {
        let _ = writeln!(out, "\n# unmanaged: {u}");
    }
    let p = spec.prune;
    let _ = write!(out, "\n[prune]\nroles = {}\ncategories = {}\nchannels = {}\n", p.roles, p.categories, p.channels);
    if has_kinds(spec) {
        for (k, v) in [
            ("automod", p.automod),
            ("emoji", p.emoji),
            ("stickers", p.stickers),
            ("sounds", p.sounds),
            ("webhooks", p.webhooks),
            ("invites", p.invites),
            ("events", p.events),
            ("messages", p.messages),
        ] {
            let _ = writeln!(out, "{k} = {v}");
        }
    }
}
