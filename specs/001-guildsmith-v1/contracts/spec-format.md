# Contract: spec file (`guild.toml`)

TOML 1.1. The JSON Schema is published as `schema/guild.schema.json` (generated, R11) and printed
by `guildsmith schema`. Field rules are in [../data-model.md](../data-model.md) §1. This file
covers syntax, defaults and compatibility.

## Example (as written by `guildsmith export`)

```toml
#:schema https://raw.githubusercontent.com/clarkscode/guildsmith/main/schema/guild.schema.json
# Exported by guildsmith 0.1.0 from "Guildsmith Lab" (123456789012345678) on 2026-10-19.

[guild]
id = "123456789012345678"
name = "Guildsmith Lab"          # informational

[[roles]]                        # listed highest first
id = "223456789012345678"
name = "Moderator"
color = "#3BA55D"
hoist = true
permissions = ["kick_members", "manage_messages", "moderate_members", "pin_messages"]

[[roles]]
name = "@everyone"
permissions = ["view_channel", "send_messages", "read_message_history"]

# managed (read-only): "Guildsmith" bot role, id 323456789012345678

[[categories]]
id = "423456789012345678"
name = "Community"

  [[categories.channels]]
  id = "523456789012345678"
  name = "general"
  kind = "text"
  topic = "Talk about anything"

  [[categories.channels]]
  name = "mod-log"
  kind = "text"
  overwrites = [
    { target = "@everyone", deny = ["view_channel"] },
    { target = "@Moderator", allow = ["view_channel"] },
  ]

[[channels]]                     # top level, no category
name = "Lobby"
kind = "voice"
user_limit = 10

[prune]
roles = false
categories = false
channels = false
```

## Rules

- **Omitted attributes are not managed.** Leaving out `topic` means "leave whatever is there";
  `topic = ""` means "clear it". Export writes every attribute it knows, so an exported file
  fully describes the server.
- **Order is meaningful** in `roles`, `categories`, `categories.channels` and `channels`. The
  order in the file becomes the order on the server, unless an item sets an explicit `position`.
- **`id` is optional.** Without it, items are matched by name within their scope (R12). Keeping
  the ids that export writes makes renames safe.
- **Permission names** are Discord's flag names in `snake_case` (e.g. `view_channel`,
  `pin_messages`, `bypass_slowmode`). `guildsmith schema` lists every name.
- **Overwrite targets:** `@everyone`, `@<role name>`, `role:<id>`, `member:<id>`.
- **Unknown keys are errors** (with a "did you mean" suggestion), so typos never pass silently.
- **Comments** in a user's file are preserved by tools that rewrite it. v1 never rewrites the
  user's file; export writes only new files.

## Compatibility

- The file has no version key in v1. Fields added later are always optional and default to "not
  managed". A breaking change to the format would add `format = 2` and keep reading format 1.
- The JSON Schema `$id` includes the major format version
  (`…/guild.schema.json` → `…/guild.v2.schema.json` for a breaking change).
