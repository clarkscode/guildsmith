# Spec file reference (`guild.toml`)

A spec file describes one Discord server: roles, categories, channels and their permission
overwrites, and optionally its settings, automod rules, onboarding, welcome screen, emoji,
stickers, sounds, webhooks, invites, scheduled events and fixed messages. `guildsmith export` writes one from a live server, `guildsmith validate`
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
| `id` | string | The server the file was exported from. Planned against another server, its items' ids and codes are treated as hints: missing items are created there. |
| `name` | string | Informational; set the name with `[guild.settings] name`. |
| `manage_settings` | bool | Turns on `[guild.settings]`. Default `false`. |

### `[guild.settings]` (only with `manage_settings = true`)

Every key is optional; omitted means not managed. Channel keys take `#name` or an id; `""` clears.

| Key | Rules |
|---|---|
| `name` | 2–100 characters. |
| `description` | At most 120 characters; `""` clears. |
| `icon`, `banner`, `splash` | A file (see *Files*). |
| `system_channel`, `rules_channel`, `public_updates_channel`, `safety_alerts_channel`, `afk_channel` | Channel references. |
| `system_channel_flags` | Names, e.g. `["suppress_join_notifications"]`. |
| `afk_timeout` | 60, 300, 900, 1800 or 3600 seconds. |
| `verification_level` | `none`, `low`, `medium`, `high`, `very_high`. |
| `default_notifications` | `all_messages`, `only_mentions`. |
| `explicit_content_filter` | `disabled`, `members_without_roles`, `all_members`. |
| `preferred_locale`, `premium_progress_bar` | |
| `features` | `{ community?, invites_disabled?, raid_alerts_disabled?, discoverable? }`. Turning Community on needs a rules channel, a public updates channel, verification `low` or higher and the content filter on `all_members`. Turning Community off or Discovery on is destructive. |

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

## Files

Emoji, stickers, sounds, server images, webhook avatars and event covers are files:
`{ file = "assets/emoji/logo.png" }` (relative to the spec file), `{ url = "https://…" }` or
`{ data = "data:image/png;base64,…" }`. Each kind has Discord's type and size limits (emoji
256 KiB; stickers 512 KiB PNG/APNG/GIF; sounds MP3/Ogg, 512 KB, at most 5.2 s). Files are
compared by blake3 of their bytes; a changed emoji, sticker or sound file is a replace (delete +
create), which is destructive. Over MCP, local files are read only under `GUILDSMITH_FILE_ROOTS`;
the CLI also reads the spec file's directory. `guildsmith export -o dir/guild.toml` saves every
file under `dir/assets/<kind>/`.

## References and ids

`#channel`, `@Role` (or plain names, or ids) resolve against the file first, so an item can use
a channel or role created by the same plan, then against the server. An unresolved reference
stops the plan. Items match by `id` (`code` for invites) first, then by name. `apply` writes the
id of every item it creates or matches by name back into the file (`--no-write-ids` to skip;
`spec_apply` does it when given `path`). Only `id = "…"` / `code = "…"` lines are added; comments
and formatting elsewhere are kept byte for byte.

## `[[automod]]`

| Key | Rules |
|---|---|
| `id`, `name` | Name 1–100 characters, unique. |
| `enabled` | Default `true`. |
| `trigger` | One of `{ keyword = { keywords?, regex?, allow? } }`, `"spam"`, `{ preset = { presets, allow? } }` (`profanity`, `sexual_content`, `slurs`), `{ mention_spam = { limit, raid_protection? } }`, `{ member_profile = { keywords?, regex?, allow? } }`. Discord's per-type rule counts and list sizes apply. A trigger type can't change: changing it replaces the rule. |
| `actions` | `{ block = { message? } }`, `{ alert = { channel } }`, `{ timeout = { duration } }` (keyword and mention-spam only, ≤ 2419200 s), `"block_interaction"`. |
| `exempt_roles`, `exempt_channels` | At most 20 roles and 50 channels. |

## `[onboarding]` and `[welcome_screen]`

Both need a Community server. Onboarding: `enabled`, `mode` (`default_channels` or `advanced`),
`default_channels` (with `enabled`, at least 7, of which at least 5 `@everyone` can send in once
the plan's changes are made), and `[[onboarding.prompts]]` with `title`, `kind`
(`multiple_choice` or `dropdown`), `single_select`, `required`, `in_onboarding` and `options`
(`title`, `description?`, `emoji?`, `roles`, `channels`). Welcome screen: `enabled`,
`description`, `channels = [{ channel, description, emoji? }]`. Each is replaced in one request
when it differs.

## `[[emoji]]`, `[[stickers]]`, `[[sounds]]`

| Kind | Keys |
|---|---|
| Emoji | `id`, `name` (2–32 of letters, digits, `_`), `image`, `roles` (who may use it). |
| Sticker | `id`, `name` (2–30), `description` (empty or 2–100), `tags`, `image`. |
| Sound | `id`, `name` (2–32), `sound`, `volume` (0–1), `emoji`. |

## `[[webhooks]]`

`id`, `name` (1–80, without "clyde" or "discord"), `channel`, `avatar`. There is no `url` or
`token` key: a new webhook's URL is shown once in the apply result (`secrets`) and never stored.
Validation rejects anything in the file shaped like a webhook URL or token.

## `[[invites]]`

`code`, `channel`, `max_age` (0–604800 seconds; 0 never expires; default 86400), `max_uses`
(0–100), `temporary`, `roles`, `recreate`. Without a `code`, an invite matches one of the bot's
invites with the same channel, `max_age`, `max_uses`, `temporary` and roles. Invites can't be
edited; an expired or deleted one is made again only with `recreate = true`.

## `[[events]]`

`id`, `name` (1–100), `description` (≤ 1000), `start`, `end`, `location`
(`{ voice = "#ch" }`, `{ stage = "#ch" }` or `{ external = { place } }`, which needs `end`),
`recurrence` (`{ frequency, interval?, weekday?, n_weekday?, month?, month_day? }`), `image`.
Times are TOML datetimes **with** an offset (`2026-10-23T20:00:00-07:00`); a local time is an
error. A scheduled event is managed fully; once it starts only `description` and `end` change;
completed, canceled and past events are reported and never made again.

## `[[messages]]`

Fixed messages the bot posts, such as a rules post: `id`, `channel`, `content` (≤ 2000),
`embeds` (≤ 10), `components_v2` (excludes `content` and `embeds`), `pinned`,
`suppress_embeds`. Only the bot's own messages are edited or deleted. Without an `id`, a message
matches one of the bot's last 100 messages in the channel with exactly that content and embeds.

## Not in a spec

Bans, members, member roles, threads, polls, voice states, stage instances and ordinary
messages change too often to describe as code; sections for them are unknown keys.

## `[prune]`

| Key | Default | Effect when `true` |
|---|---|---|
| `roles` | `false` | Delete roles the spec doesn't list. Never `@everyone`, managed roles, or roles at or above the bot's highest role. |
| `categories` | `false` | Delete unlisted categories, but only ones the plan empties (otherwise their channels would spill into the top level). |
| `channels` | `false` | Delete unlisted channels of the kinds guildsmith manages. Unsupported kinds (threads, directories) are never touched. |

| `automod`, `emoji`, `stickers`, `sounds`, `webhooks`, `invites`, `events`, `messages` | `false` | Delete unlisted items of that kind. Invites: only the bot's. Events: only scheduled ones. Messages: only the bot's, in channels the spec's messages use. |

Deletes are destructive and always run last, in reverse dependency order.

## Example

See [`examples/`](../examples): `community.toml`, `dev-team.toml` and `gaming.toml` each validate,
apply to an empty server, and plan to no changes afterwards. `full-server/guild.toml` has one
item of every kind, with its files in `full-server/assets/`.

## Compatibility

There is no version key. Fields added later are optional and default to "not managed", so a v1
file means the same thing today. A
breaking change would add `format = 2` and keep reading format 1, and the schema `$id` would move
to `guild.v2.schema.json`.
