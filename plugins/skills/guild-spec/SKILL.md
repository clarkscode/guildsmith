---
name: guild-spec
description: How to read, write and edit a guildsmith server file (guild.toml) describing a Discord server's roles, categories, channels and permission overwrites. Use when creating or changing a guild.toml.
user-invocable: false
---

# Writing guild.toml

A `guild.toml` file describes the structure of one Discord server: roles, categories, channels and
permission overwrites. Start from a real server with `spec_export` (or `guildsmith export -g <server>`)
rather than writing one from scratch, and check every edit with `spec_validate`
(`guildsmith validate <file>`). `guildsmith schema` prints the JSON Schema.

## Shape

```toml
[guild]                      # informational only; never changes the server
id = "123456789012345678"
name = "My Server"

[[roles]]                    # listed highest first
id = "223456789012345678"    # keep ids that export writes: renames stay safe
name = "Moderator"
color = "#3BA55D"
hoist = true
mentionable = false
permissions = ["kick_members", "manage_messages", "moderate_members"]

[[roles]]
name = "@everyone"           # permissions only
permissions = ["view_channel", "send_messages", "read_message_history"]

[[categories]]
name = "Community"
overwrites = []              # [] = no overwrites; omitted = not managed

  [[categories.channels]]
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

[[channels]]                 # top level, no category
name = "Lobby"
kind = "voice"
user_limit = 10

[prune]                      # all false by default: unlisted items are left alone
roles = false
categories = false
channels = false
```

## Rules

- **Omitted means unmanaged.** Leaving out `topic` keeps whatever is on the server; `topic = ""`
  clears it. The same holds for every optional attribute, including `overwrites`.
- **Order is meaningful** in `roles`, `categories`, `categories.channels` and `channels`, unless an
  item sets `position`.
- **Matching**: by `id` first, then by exact name in scope (roles and categories per server,
  channels per parent category). Names must be unique in their scope. To move a channel to
  another category, keep its `id`: without one, a name only matches inside its own category, so
  the plan would create a new channel instead.
- **Kinds**: `text`, `announcement`, `voice`, `stage`, `forum`, `media`. Changing kind other than
  text ↔ announcement deletes and recreates the channel; it needs `replace = true` on the channel
  or `prune.channels = true`.
- **Overwrite targets**: `@everyone`, `@RoleName`, `role:<id>`, `member:<id>`. A permission may not
  be in both `allow` and `deny`; each target appears once per channel.
- **`sync_permissions = true`** on a channel copies its category's overwrites; leave `overwrites`
  empty or omitted.
- **Permission names** are Discord flags in `snake_case`: `view_channel`, `send_messages`,
  `manage_roles`, `pin_messages`, `bypass_slowmode`, …
- **Text-like names** (text, announcement, forum, media) are lowercase with `-` for spaces, as
  Discord stores them.
- **`@everyone`** may only set `permissions`. **Managed roles** (bots, integrations, boosters)
  appear as comments; list one only with `managed = true`, and then only `permissions` may change.
- `[guild] manage_settings = true` is rejected: server settings aren't managed in v1.
- Unknown keys are errors with a "did you mean" hint.

## Limits

250 roles; 500 channels including categories; 50 channels per category; names 1–100 characters;
topic ≤ 1024 (≤ 4096 for forum/media, none on voice or stage); slowmode 0–21600 s (not on announcement);
bitrate ≥ 8000; `user_limit` 0–99 for voice, ≤ 10000 for stage; forum `tags` ≤ 20, names ≤ 20
characters.

To apply a file, follow the guild-safe-apply skill.
