//! Deterministic spec file writer (research R11). Values are encoded by `toml_edit`, so quoting
//! and escaping follow TOML exactly; the layout (order, comments, inline overwrite tables) is
//! fixed so exporting twice gives byte-identical files.

use std::fmt::Write as _;

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

    out.push_str("\n[guild]\n");
    if let Some(id) = spec.guild.id {
        let _ = writeln!(out, "id = {}", q(&id.to_string()));
    }
    if let Some(n) = &spec.guild.name {
        let _ = writeln!(out, "name = {}  # informational", q(n));
    }

    if !spec.roles.is_empty() {
        out.push_str("\n# Roles are listed highest first.");
    }
    for r in &spec.roles {
        write_role(&mut out, r, notes);
    }
    for m in &notes.managed_roles {
        let _ = writeln!(out, "\n# managed (read-only): {m}");
    }
    for cat in &spec.categories {
        write_category(&mut out, cat);
    }
    for ch in &spec.channels {
        write_channel(&mut out, "channels", "", ch);
    }
    for u in &notes.unmanaged {
        let _ = writeln!(out, "\n# unmanaged: {u}");
    }
    let p = spec.prune;
    let _ = write!(out, "\n[prune]\nroles = {}\ncategories = {}\nchannels = {}\n", p.roles, p.categories, p.channels);
    out
}
