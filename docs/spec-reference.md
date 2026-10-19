# Spec file reference (`guild.toml`)

A spec file describes the structure of one Discord server: roles, categories, channels and their
permission overwrites. `guildsmith export` writes one from a live server, `guildsmith validate`
checks one offline, and `guildsmith plan` / `apply` make a server match it.

The file is TOML 1.1. Its JSON Schema is `schema/guild.schema.json` (also printed by
`guildsmith schema`). Exported files start with a `#:schema` line, so Taplo and Even Better TOML
give autocompletion.

## Core rules

- **Omitted attributes are not managed.** Leaving out `topic` keeps whatever the server has;
  `topic = ""` clears it. This holds for every optional attribute, including `overwrites`
  (omitted = not managed, `[]` = no overwrites).
- **Order is meaningful** in `roles` (first = highest), `categories`, `categories.channels` and
  `channels`. The plan reorders the server to match, unless an item sets an explicit `position`.
- **Matching**: by `id` first, then by exact name within the item's scope: roles and categories
  per server, channels per parent category (or the top level). Two `general` channels in different
  categories don't conflict. To move a channel to another category, keep its `id`.
- **No state file.** Unlisted items are reported as unmanaged and left alone unless `[prune]`
  turns deletion on for that kind.
- **Unknown keys are errors**, with a "did you mean" suggestion, and every problem reports
  `file:line:col`.

## `[guild]`

| Key | Type | Notes |
|---|---|---|
| `id` | string | Informational. |
| `name` | string | Informational: server settings aren't managed in v1. |
| `manage_settings` | bool | Reserved. `true` is rejected in v1. |

## `[[roles]]`

| Key | Type | Rules |
|---|---|---|
| `id` | string | Written by export; makes renames safe. |
| `name` | string | 1–100 characters, unique. `@everyone` is the default role and may only set `permissions`. |
| `color` | `"#RRGGBB"` | The primary color. Gradient colors are read-only and appear as comments. |
| `hoist` | bool | Show members separately. |
| `mentionable` | bool | |
| `permissions` | list of names | The role's full permission set. |
| `position` | integer | Optional explicit position; without it, file order decides. |
| `managed` | bool | Must match the live role. Roles from bots, integrations or boosts can only be listed with `managed = true`, and then only `permissions` may change. |

At most 250 roles. `@everyone` is never created, deleted or moved.

## `[[categories]]`

| Key | Type | Rules |
|---|---|---|
| `id` | string | |
| `name` | string | 1–100 characters, unique. |
| `position` | integer | Optional explicit position. |
| `overwrites` | list | Permission overwrites (below). |
| `channels` | `[[categories.channels]]` | At most 50 per category. |

## Channels: `[[categories.channels]]` and top-level `[[channels]]`

| Key | Type | Applies to | Rules |
|---|---|---|---|
| `id` | string | all | |
| `name` | string | all | 1–100 characters, unique within the parent. Text-like kinds are stored lowercase with `-` for spaces; validation warns when a name would be rewritten. |
| `kind` | string | all | `text`, `announcement`, `voice`, `stage`, `forum` or `media`. |
| `topic` | string | text, announcement, forum, media | 0–1024 characters, or 0–4096 for forum and media. Not allowed on voice or stage. |
| `nsfw` | bool | all | |
| `slowmode` | seconds | all but announcement | 0–21600. |
| `bitrate` | integer | voice, stage | ≥ 8000. |
| `user_limit` | integer | voice, stage | 0–99 for voice (0 = unlimited), ≤ 10000 for stage. |
| `rtc_region` | string | voice, stage | `"auto"` lets Discord choose. |
| `tags` | list of `{ name, emoji?, moderated? }` | forum, media | At most 20; names ≤ 20 characters. |
| `default_reaction` | string | forum, media | A unicode emoji or a custom emoji id; `""` clears it. |
| `sync_permissions` | bool | in a category | The channel gets its category's overwrites; leave `overwrites` out. |
| `overwrites` | list | all | Permission overwrites (below). |
| `replace` | bool | all | Allows a kind change by delete + create (destructive). |

At most 500 channels including categories. Changing kind in place is possible only for
text ↔ announcement, on servers with the `NEWS` feature. Any other kind change is a **replace**
(delete and recreate, losing messages), allowed only with `replace = true` on the channel or
`prune.channels = true`.

## Permission overwrites

```toml
overwrites = [
  { target = "@everyone", deny = ["view_channel"] },
  { target = "@Moderator", allow = ["view_channel", "manage_messages"] },
  { target = "member:123456789012345678", allow = ["view_channel"] },
]
```

- `target` is `@everyone`, `@<role name>` (a role in the spec), `role:<id>` or `member:<id>`.
- Each target appears once per channel. A permission may not be in both `allow` and `deny`.
- The plan sends a channel's full overwrite list together with its other changes, in one request.

## Permission names

Discord's permission flags in `snake_case`, for example `view_channel`, `send_messages`,
`read_message_history`, `manage_messages`, `manage_roles`, `manage_channels`, `kick_members`,
`ban_members`, `moderate_members`, `pin_messages`, `bypass_slowmode`, `connect`, `speak`,
`administrator`. `guildsmith schema` lists every name. Bits guildsmith doesn't know are kept as
`unknown_<bit>`, so a round trip never loses data.

## `[prune]`

| Key | Default | Effect when `true` |
|---|---|---|
| `roles` | `false` | Delete roles the spec doesn't list. Never `@everyone`, managed roles, or roles at or above the bot's highest role. |
| `categories` | `false` | Delete unlisted categories, but only ones the plan empties (otherwise their channels would spill into the top level). |
| `channels` | `false` | Delete unlisted channels of the kinds guildsmith manages. Unsupported kinds (threads, directories) are never touched. |

Deletes are destructive and always run last.

## Example

See [`examples/`](../examples): `community.toml`, `dev-team.toml` and `gaming.toml` each validate,
apply to an empty server, and plan to no changes afterwards.

## Compatibility

There is no version key in v1. Fields added later are optional and default to "not managed". A
breaking change would add `format = 2` and keep reading format 1, and the schema `$id` would move
to `guild.v2.schema.json`.
