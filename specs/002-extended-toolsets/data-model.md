# Data Model: Extended toolsets

**Plan**: [plan.md](plan.md) · **Research**: [research.md](research.md) · Extends
[001 data model](../001-guildsmith-v1/data-model.md), referred to as `v1-DM§n`.

Types live in `guildsmith-core` (pure) unless marked **app** (`guildsmith-app`). Limits come
from research E4.

## 1. Tool groups and dispatch (app)

### Toolset
| Field | Notes |
|---|---|
| `name` | `core`, `moderation`, `messages`, `threads`, `polls`, `events`, `invites`, `voice`, `automod`, `webhooks`, `expressions`, `settings`, `raw` |
| `tools` | static list of tool definitions, in a fixed order |
| `required_permissions` | `PermissionSet`, plus required intents (e.g. MESSAGE_CONTENT) |
| `approx_tokens` | computed at build time (v1-R16 method) |

### ToolsetState (per process)
`registered: BTreeSet<Toolset>` (operator, at startup; shown in `tools/list`),
`unlocked: BTreeSet<Toolset>` (registered plus anything the agent enabled; callable through
`tool_call`), `blocked: BTreeSet<Toolset>` (operator).

The agent may change `unlocked` only. `blocked`, the allowlist and read-only mode are read-only
to every tool.

### Enablement (log record)
`{ at: Timestamp, toolset, by: Operator | Agent }`, written to stderr at `info` level.

**Transitions**: `locked → unlocked` (via `toolsets(enable)`, refused if blocked). There is no
way back within a process.

## 2. Bulk actions (app)

### BulkAction<T>
| Field | Notes |
|---|---|
| `targets` | `Vec<T>` (members, message ids, users) |
| `problems` | `Vec<(T, Problem)>`: role at or above the bot, owner, self, too old, not in voice |
| `batch_size` | 200 for bans, 100 for bulk delete, 1 otherwise |
| `estimated_requests` | |

### BulkOutcome<T>
`succeeded: Vec<T>`, `failed: Vec<(T, String)>`, `not_attempted: Vec<T>`. Invariant:
`|succeeded| + |failed| + |not_attempted| == |targets|` (SC-005).

**States**: `Previewed → Running → (Completed | Cancelled | Stopped(error))`. Each final state
produces a `BulkOutcome`.

## 3. Raw requests

### RawRequest (core, pure)
`method`, `path` (relative to `/api/v10/`, with no leading slash), `query: Vec<(k, v)>`,
`body: Option<Json>`.

### RawScope (result of `classify(path, method)`)
`Guild(Id)` | `Channel(Id)` (needs a lookup) | `Webhook(Id)` (needs a lookup) |
`Invite(code)` (needs a lookup) | `SelfUser` (GET only) | `Refused(reason)`. The refused list is
in research E2. **app** resolves a lookup to a server id, then applies the policy: allowlist,
read-only, confirmation and audit reason.

### RawResponse
`status`, `json` (with tokens redacted), `truncated: bool`, `bytes`.

## 4. Spec file: new kinds (extends v1-DM§1)

New optional top-level fields on `GuildSpec`: `settings` (in `[guild.settings]`), `automod`,
`onboarding`, `welcome_screen`, `emoji`, `stickers`, `sounds`, `webhooks`, `invites`, `events`,
`messages`. `PruneSpec` gains one flag per kind (all `false`).

### SettingsSpec (used only when `guild.manage_settings = true`)
`name`, `description`, `icon: Asset?`, `banner: Asset?`, `splash: Asset?`, `system_channel`,
`system_channel_flags`, `rules_channel`, `public_updates_channel`, `safety_alerts_channel`,
`afk_channel`, `afk_timeout` (60/300/900/1800/3600), `verification_level`,
`default_notifications`, `explicit_content_filter`, `preferred_locale`, `premium_progress_bar`,
`features: { community?, invites_disabled?, raid_alerts_disabled?, discoverable? }`.

Channel fields are **references** (`#name` or an id), resolved within the same spec.

### AutomodRuleSpec
`id?`, `name`, `enabled`, `trigger` (one of `keyword { keywords, regex, allow }`,
`spam`, `preset { presets, allow }`, `mention_spam { limit, raid_protection }`,
`member_profile { keywords, regex, allow }`), `actions: [block { message? } | alert { channel } |
timeout { duration } | block_interaction]`, `exempt_roles` (≤ 20), `exempt_channels` (≤ 50).

**Validation**: per-type rule counts, list sizes and lengths (E4). `timeout` is allowed only on
keyword and mention-spam rules.

### OnboardingSpec / WelcomeScreenSpec
Onboarding: `enabled`, `mode` (`default_channels` | `advanced`), `default_channels: [ref]`, and
`prompts: [{ id?, title, kind (multiple_choice | dropdown), single_select, required,
in_onboarding, options: [{ id?, title, description?, emoji?, roles: [ref], channels: [ref] }] }]`.
**Validation**: if enabled, at least 7 default channels, at least 5 of which `@everyone` can send
in. This is computed with v1's explain function over the spec's own overwrites.

Welcome screen: `enabled`, `description`, `channels: [{ channel: ref, description, emoji? }]`.

### Asset
`file: Path` (relative to the spec file) | `url: Url` | `data: Base64`. Validated for type and
size per kind (E4). Compared by blake3 of the bytes, or decoded pixels if the verification in E7
fails.

### EmojiSpec / StickerSpec / SoundSpec
- Emoji: `id?`, `name` (2–32, `[A-Za-z0-9_]`), `image: Asset`, `roles: [ref]` (who may use it).
- Sticker: `id?`, `name` (2–30), `description` (empty or 2–100), `tags` (≤ 200), `image: Asset`.
- Sound: `id?`, `name` (2–32), `sound: Asset`, `volume` (0–1), `emoji?`.

### WebhookSpec
`id?`, `name` (1–80, no "clyde"/"discord"), `channel: ref`, `avatar: Asset?`. **No token or URL
fields exist.** Validation rejects strings shaped like webhook URLs anywhere in the file (E8).

### InviteSpec
`code?`, `channel: ref`, `max_age` (0–604800; 0 = never expires), `max_uses` (0–100),
`temporary`, `roles: [ref]`, `recreate: bool = false`.
**Signature** for matching without a code: `(channel, max_age, max_uses, temporary,
sorted(roles))`. Only invites created by the bot are considered.

### EventSpec
`id?`, `name`, `description?`, `start: DateTime<offset>`, `end?`, `location` (one of
`voice(ref)`, `stage(ref)`, `external { place }`; external needs an `end`),
`recurrence?: { frequency, interval, weekday | n_weekday | month_day }`, with E4's limits.
`image: Asset?`.
**States as Discord reports them**: `scheduled → active → completed`, or `→ canceled`. The
planner manages `scheduled` events fully, `active` events partly (description and end only), and
reports `completed`/`canceled`/past events and leaves them alone.

### FixedMessageSpec
`id?`, `channel: ref`, `content?`, `embeds: [Embed]`, `components_v2?: Json`, `pinned: bool`,
`suppress_embeds?`.
**Rules**: the message must have been sent by the bot. With no id, it is matched by exact
`(content, embeds)` among the bot's last 100 messages in the channel. If `components_v2` is set,
`content` and `embeds` must be empty.

## 5. Ops (extends v1-DM§3)

New `Op` variants, each with a summary and a risk level:

| Op | Risk |
|---|---|
| `UpdateSettings { changes }` | safe, except turning off `community` or turning on `discoverable` (destructive) |
| `Create`/`Update`/`DeleteAutomod` | create/update safe; delete destructive (prune) |
| `SetOnboarding`, `SetWelcomeScreen` | safe (one PUT/PATCH each) |
| `Create`/`Update`/`Replace`/`DeleteExpression { kind }` | replace and delete destructive |
| `Create`/`Update`/`DeleteWebhook` | delete destructive; create reports a secret (E8) |
| `CreateInvite`, `DeleteInvite` | delete destructive (prune); recreating an expired invite is safe |
| `Create`/`Update`/`DeleteEvent` | delete destructive |
| `PostMessage`, `EditMessage`, `Pin`/`UnpinMessage`, `DeleteMessage` | delete destructive |
| `WriteIds { items }` | local only; applied to the spec file at the end (E9) |

**Ordering** (extends v1-R8):
1. v1 structure ops (roles → categories → channels → positions).
2. Settings, which may refer to channels.
3. Expressions.
4. Webhooks.
5. Automod.
6. Onboarding and welcome screen, which refer to roles and channels.
7. Events.
8. Invites.
9. Fixed messages and pins.
10. All deletes, in reverse dependency order.
11. `WriteIds`.

**Convergence** (property-tested against an extended `FakeDiscord`):
`diff(spec, apply(spec, s)) = ∅` for every kind. Invites without a code and messages without an
id converge through their signature or content match.

## 6. Tool output additions

- Every output with user-written text (messages, thread names, poll answers, event
  descriptions, member names, raw responses) uses v1-R19 wrapping.
- `secrets` appears only in `webhook_create` and `spec_apply` results, and is never cached.
