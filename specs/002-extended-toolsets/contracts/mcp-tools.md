# Contract: extended MCP tools

Extends [001 mcp-tools.md](../../001-guildsmith-v1/contracts/mcp-tools.md). Its shared
conventions apply unchanged: `guild`, policy refusals as results, name resolution, paging,
`structuredContent` plus a JSON text block, `confirm` previews, `reason` for the audit log.

**Annotations**: RO = read-only. W = write, additive. D = destructive (previews until
`confirm`). I = idempotent. **Bulk**: the tool uses BulkAction (data-model §2): preview, then
progress, then a `{succeeded, failed, not_attempted}` report.

## Dispatcher (added to the default set: 21 + 2 = 23)

| Tool | Ann. | Inputs | Output |
|---|---|---|---|
| `toolsets` | RO\* | `action: list \| describe \| enable`, `toolset?` | list: `[{name, purpose, tools, approx_tokens, required_permissions, registered, unlocked, blocked}]`. describe: the group's tool definitions (name, description, inputSchema, annotations). enable: `{unlocked: true}` or a refusal. |
| `tool_call` | D (hint) | `tool`, `arguments` | the target tool's result, unchanged. Refused if the tool's group isn't unlocked. Policy is applied as for a direct call. |

\* `enable` changes guildsmith's own state, not Discord's. The tool is marked read-only because
it can't touch the server; the enablement is logged.

## `moderation` (10, replaces v1's 4)

| Tool | Ann. | Inputs |
|---|---|---|
| `member_ban` | D, Bulk | `members: [member]` (1–N, sent in batches of 200), `delete_message_seconds?` (≤ 604800), `reason?` |
| `ban_remove` | W | `users: [user]`, `reason?` |
| `bans_list` | RO | paging (`limit` ≤ 1000, `before`/`after`) |
| `member_timeout` | W, I, Bulk | `members`, `duration` (≤ 28 d; `0` clears) |
| `member_kick` | D, Bulk | `members`, `reason?` |
| `members_prune` | D | `days` (1–30), `roles?`; preview = Discord's prune count |
| `messages_purge` | D, Bulk | `channel`, `author?`, `count?` (≤ 1000), `after?`/`before?`, `include_old: bool = false` (E10) |
| `roles_bulk_assign` | W, Bulk | `role`, `add?: [member]`, `remove?: [member]` |
| `audit_log` | RO | `action_type?`, `user?`, `before?`, `limit?` (≤ 100) |
| `raid_lockdown` | D | `pause_invites_for?` (≤ 24 h), `pause_dms_for?` (≤ 24 h), or `clear: true` |

## `messages` (9)

| Tool | Ann. | Inputs |
|---|---|---|
| `message_compose` | W | `channel`, `content?`, `embeds?`, `attachments?: [Asset]`, `reply_to?`, `forward?`, `components_v2?`, `allow_mentions?: {users, roles, everyone: false}` |
| `message_edit` | W, I | `channel`, `message`, `content?`, `embeds?`, `suppress_embeds?` (only content the bot authored; flags otherwise) |
| `message_delete` | D | `channel`, `message` |
| `message_pin` | W, I | `channel`, `message`, `pinned: bool` |
| `pins_list` | RO | `channel`, paging |
| `message_crosspost` | W | `channel`, `message` |
| `reactions` | W | `channel`, `message`, `action: add \| remove \| clear`, `emoji?`, `user?`. `clear` is D. |
| `reactions_list` | RO | `channel`, `message`, `emoji`, paging |
| `messages_search` | RO | `content?`, `author?`, `channels?`, `has?`, `before?`/`after?`, `scan: bool = false`, paging (E11) |

## `threads` (6)

| Tool | Ann. | Inputs |
|---|---|---|
| `thread_create` | W | `channel`, `name`, `from_message?`, `private?`, `auto_archive?`, `tags?` and `message?` (forum/media post) |
| `thread_update` | W, I | `thread`, `name?`, `archived?`, `locked?`, `auto_archive?`, `tags?`, `pinned?`, `slowmode?` |
| `thread_delete` | D | `thread` |
| `threads_list` | RO | `channel?`, `state: active \| archived_public \| archived_private`, paging |
| `thread_members` | W | `thread`, `action: list \| add \| remove`, `member?` |
| `forum_tags` | W | `channel`, `action: create \| rename \| delete \| default_reaction`, `tag?`, `name?`, `emoji?`, `moderated?`. `delete` is D. |

## `polls` (3)

`poll_create` (W: `channel`, `question` ≤ 300, `answers` (2–10, each ≤ 55), `duration` ≤ 768 h,
`multiselect?`) · `poll_results` (RO: `channel`, `message`, `voters_for?`, paging) ·
`poll_end` (W: `channel`, `message`; only polls the bot sent).

## `events` (5)

`event_create` (W: `name`, `start`, `end?`, `tz?`, `location`, `description?`, `recurrence?`,
`image?`) · `event_update` (W, I: `event`, any field, `status?: active | completed | canceled`;
canceling is D) · `event_delete` (D) · `events_list` (RO, with interested counts) ·
`event_users` (RO, paging).

## `invites` (4)

`invite_create` (W: `channel`, `max_age?`, `max_uses?`, `temporary?`, `roles?`,
`only_users?: [user]`) · `invites_list` (RO: `channel?`, shows uses and expiry) ·
`invite_revoke` (D: `code`) · `invite_target_users` (W: `code`, `action: list | add | remove`,
`users?`).

## `voice` (4)

`voice_members` (W, Bulk: `members` (≤ 25), `move_to?`, `disconnect?`, `mute?`, `deafen?`;
members not in voice are reported, E5) · `voice_state` (RO: `member`) ·
`stage` (W: `channel`, `action: start | update | end`, `topic?`, `notify?`; `end` is D) ·
`voice_channel_status` (W, I: `channel`, `status` ≤ 500).

## `automod` (3)

`automod_rules_list` (RO) · `automod_rule_upsert` (W, I: `rule?`, the fields of
data-model §4 `AutomodRuleSpec`) · `automod_rule_delete` (D: `rule`). Disabling a rule through
upsert (`enabled = false`) also needs `confirm`.

## `webhooks` (5)

`webhook_create` (W: `channel`, `name`, `avatar?`) returns `secrets: {url}` **once** ·
`webhooks_list` (RO: `channel?`; no tokens or urls) · `webhook_update` (W, I: `webhook`,
`name?`, `avatar?`, `channel?`) · `webhook_delete` (D) · `webhook_send` (W: `webhook`, message
fields as in `message_compose`, `thread?`, `username?`, `avatar_url?`). guildsmith gets the
webhook's token itself, from Discord (`GET webhooks/{id}` with MANAGE_WEBHOOKS, *unverified* that
the token is returned for incoming webhooks) or from memory for webhooks created in this
process. The token never appears in inputs, outputs or logs.

## `expressions` (4)

`expressions_list` (RO: `kind?: emoji | sticker | sound`; includes `slots: {used, approx_max}`) ·
`expression_create` (W: `kind`, `name`, `asset`, kind-specific fields) · `expression_update`
(W, I: `kind`, `item`, `name?`, `roles?`, `tags?`, `volume?`) · `expression_delete` (D).

## `settings` (7)

| Tool | Ann. | Inputs |
|---|---|---|
| `settings_get` | RO | `section?: general \| onboarding \| welcome_screen \| widget \| integrations \| vanity` |
| `settings_update` | D | any `SettingsSpec` field; the preview shows old → new |
| `onboarding_update` | D | `OnboardingSpec` (full replace, validated first) |
| `welcome_screen_update` | W, I | `WelcomeScreenSpec` |
| `templates` | W | `action: list \| create \| sync \| update \| delete`, `name?`, `description?`; `delete` is D |
| `template_import` | RO | `code_or_url`, `path?`. Returns a spec file (FR-035). Never creates a server. |
| `integration_remove` | D | `integration` |

## `raw` (1)

`discord_request` (D (hint): `method`, `path`, `query?`, `body?`, `confirm`, `reason?`). The
rules are in research E2. GET requests run without `confirm`. Every other method previews until
confirmed.

## Spec tools (changes to v1)

- `spec_export` gains `assets_dir?` (needs `GUILDSMITH_FILE_ROOTS`, E7) and exports every kind
  listed in the [spec-format additions](spec-format.md).
- `spec_apply` gains `path?`. When given, created ids are written back into that file (E9). The
  result gains `secrets` (E8) and `written_ids`.

## Budgets

Each optional group: ≤ 15 tools and ≤ 1,500 description tokens. Default: ≤ 25 tools and
≤ 3,000 tokens. `all`: ≤ 15,000 description tokens. Checked by `cargo xtask budgets`.
