# Research: Extended toolsets

**Date**: 2026-10-19 · **Plan**: [plan.md](plan.md) · **Spec**: [spec.md](spec.md) ·
**Builds on**: [001 research](../001-guildsmith-v1/research.md), referred to as `v1-R<n>`

Sources: docs.discord.com/developers (the docs repo at 2026-10-16) and its change log; the
twilight-http 0.17.1 source and twilight `main` (2026-10-18); the MCP 2026-07-28 spec; the
Claude Code docs; the openai/codex source (2026-10-19); the github-mcp-server history; rmcp
3.5.1. Items marked *(unverified)* must be checked during implementation.

---

## E1. How the agent turns on tool groups (FR-003a–c, US8)

- **Decision**: The tool list stays **static for the life of the process**. "Enabling" a group
  unlocks it for dispatch; it does not change `tools/list`.
  - **At startup**: groups chosen by the operator (`GUILDSMITH_TOOLSETS`, including `all`) are
    registered as normal tools.
  - **Always present**: two **dispatcher tools** join the default set:
    - `toolsets` (`action = list | describe | enable`) lists groups with their tool counts,
      context cost and required permissions, returns a group's tool schemas, and enables a
      group.
    - `tool_call` (`tool`, `arguments`) runs any tool in an enabled group and returns that
      tool's normal result.
  - **Guards**: groups the operator blocks (`GUILDSMITH_TOOLSETS_BLOCK`) can't be enabled.
    Every enablement is logged to stderr (FR-003c). An enablement lasts until the process
    exits.
- **Rationale**:
  1. MCP 2026-07-28 says the tool list "MUST NOT vary … as a side effect of other requests on
     the connection". An `enable` call that grew `tools/list` would be exactly that.
  2. Codex ignores `tools/list_changed` by default (the `mcp_tool_list_changed` feature flag is
     off), so newly added tools would never appear there.
  3. GitHub removed its similar "dynamic toolsets" feature on 2026-05-20; it had no fallback
     for clients that ignore list changes.
  4. A dispatcher works in every client, which satisfies FR-003a's "even if the client can't
     refresh".
- **Cost**: the default set becomes **23 tools** (v1's 21 plus 2). That's still within the
  constitution's limit of 25 and the 3k-token description budget. The spec's SC-002 is updated
  to match.
- **Operator "all"**: registering ~85 tools up front is cheap in Claude Code and Codex, which
  load MCP tools lazily through tool search. Other clients pay the full cost, which startup
  reports (FR-010).
- **Approvals**: an "always allow" on `tool_call` in a client would cover every tool behind it.
  The real safety controls stay inside guildsmith: previews until confirmed, the allowlist,
  read-only mode and audit reasons. The skills docs explain this trade-off.
- **Alternatives**: changing the tool list at runtime plus `list_changed` (rejected for the
  reasons above); Cloudflare-style generated code (needs a code sandbox, overkill).

## E2. Raw requests (`raw` group, FR-036a–d)

- **Decision**: One tool, `discord_request(method, path, query?, body?, confirm?, reason?)`. It
  is sent with twilight's `RequestBuilder::raw(Method, path)` and `client.request()`, which
  **uses the same auth and rate limiter as every other call**. The rate limiter keys requests by
  `(method, path)` and the bucket header, so raw calls can't get around the global limit.
- **Scoping** (a pure classifier in `guildsmith-core::raw`): the path must tie to an allowed
  server.

  | Path | How the server is found |
  |---|---|
  | `guilds/{id}/…` | the id itself |
  | `channels/{id}/…`, `stage-instances/{channel}` | the channel's server, from the snapshot or one GET |
  | `webhooks/{id}` (no token) | the webhook's `guild_id` |
  | `invites/{code}/…` | the invite's guild |
  | `users/@me` (GET only), `users/@me/guilds/{id}` (DELETE = the bot leaves, destructive) | no lookup needed / the id itself |

  **Refused**: `webhooks/{id}/{token}…` (tokens stay out of agent traffic; use the `webhooks`
  group), `applications/…/commands` (out of scope: guildsmith is not a bot framework),
  `oauth2/…`, `gateway…`, `POST guilds`, and anything that can't be resolved.
- **Policy**:
  - Every method other than GET is a write: preview `{method, path, query, body}` unless
    `confirm`, refused in read-only mode, `X-Audit-Log-Reason` added (percent-encoded by us,
    since twilight's helper is private), snapshot cache cleared.
  - A `body` may not contain an `Authorization` header or a token.
- **Response**:
  - JSON is returned with any `token` field removed and webhook `url`s replaced by
    `"<redacted>"`.
  - It's cut to fit 50,000 characters, with a note when cut.
  - The whole body is wrapped as untrusted content (v1-R19).
  - The tool description tells the agent to prefer a dedicated tool when one exists (FR-036d).
- **Constitution**: this goes against principle IV ("never raw Discord JSON"). See the plan's
  Complexity Tracking, and recommend a MINOR amendment.

## E3. Endpoints twilight 0.17.1 lacks

- **Decision**: Add them in `guildsmith-discord/src/ext/`, each as a small serde request and
  response type sent through the same `RequestBuilder::raw` path as E2, so they get the same
  limiter, auth and audit header.

  | Missing in 0.17.1 | Ext module |
  |---|---|
  | bulk ban (`POST guilds/{g}/bulk-ban`) | `ext/bans.rs` |
  | soundboard sounds (CRUD; own model) | `ext/soundboard.rs` |
  | voice channel status (`PUT channels/{c}/voice-status`) | `ext/voice.rs` |
  | guild message search (`GET guilds/{g}/messages/search`) | `ext/search.rs` |
  | scheduled event `recurrence_rule` | `ext/events.rs` (create/update bodies) |
  | automod: `mention_raid_protection_enabled`, member-profile trigger, block-interaction action, **and the update body that sends nulls** | `ext/automod.rs` (our own create/update bodies) |
  | guild `description`, `safety_alerts_channel_id` | `ext/guild.rs` |
  | invite `role_ids`, target-user endpoints | `ext/invites.rs` |

- **Rationale**: Each is 20–60 lines. Open twilight PRs exist for most of them, so each ext
  module is deleted once a twilight release covers it, with a test that checks the payload is
  the same.

## E4. Discord limits used for validation and previews

| Area | Limits and rules (docs.discord.com, 2026-10) |
|---|---|
| Bulk ban | at most 200 users per request; `delete_message_seconds` ≤ 604800; needs BAN_MEMBERS **and** MANAGE_GUILD |
| Bans list | `limit` ≤ 1000, `before`/`after` |
| Timeout | ≤ 28 days; not on admins or the owner |
| Prune | `days` 1–30; `include_roles`; `compute_prune_count=false` for large servers; ADMINISTRATOR if the server has `PRUNE_REQUIRES_ADMIN` |
| Bulk delete | 2–100 ids; the **whole request fails** if any id is older than 14 days, so the planner splits first |
| Pins | at most 250 per channel; needs PIN_MESSAGES; uses the new paginated `messages/pins` routes |
| Uploads | 20 MiB per file, 25 MiB per request |
| Components V2 | flag `1<<15`; no `content`/`embeds`/`poll`; ≤ 40 components; the flag can't be removed |
| Search | `limit` ≤ 25, `offset` ≤ 9975; needs the **MESSAGE_CONTENT privileged intent**; a `202` means "not indexed yet", retry after `retry_after` |
| Threads | names 1–100; auto-archive 60/1440/4320/10080; forum tags ≤ 20 per forum, ≤ 5 per post |
| Polls | ≤ 10 answers; question ≤ 300 chars, answers ≤ 55; duration ≤ 768 h; SEND_POLLS; only the bot's own polls can be ended; no edits |
| Events | ≤ 100 scheduled or active; CREATE_EVENTS; external events need location + end; recurrence limits (no count/end, one by-* entry, interval only for weekly) |
| Invites | `max_age` ≤ 604800, `max_uses` ≤ 100, `role_ids` (MANAGE_ROLES, below the bot); ≤ 1000 per server |
| Voice | move/disconnect (MOVE_MEMBERS), mute/deaf (only when the member is in voice); stage topic 1–120; voice status ≤ 500 chars (SET_VOICE_CHANNEL_STATUS) |
| Automod | keyword rules ≤ 6, other types 1 each; keywords ≤ 1000 × 60 chars; regex ≤ 10 × 260; allow list ≤ 100 (1000 for presets); mention limit ≤ 50; exempt roles ≤ 20, channels ≤ 50; timeout action ≤ 2419200 s |
| Webhooks | names 1–80, no "clyde" or "discord"; ≤ 15 per channel, ≤ 1000 per server |
| Emoji | 256 KiB, JPEG/PNG/GIF/WebP/AVIF; CREATE_GUILD_EXPRESSIONS / MANAGE_GUILD_EXPRESSIONS |
| Stickers | 512 KiB, PNG/APNG/GIF (Lottie only on verified/partnered servers); name 2–30 |
| Sounds | MP3/Ogg, 512 KB, ≤ 5.2 s; name 2–32 |
| Slots by boost tier | *(unverified, from support articles)* emoji 50/100/150/250 (static and animated each), stickers 5/15/30/60, sounds 8/24/36/48. Shown as "approximately"; Discord's own error wins. |
| Onboarding | needs MANAGE_GUILD + MANAGE_ROLES; when enabled, ≥ 7 default channels with ≥ 5 that @everyone can send in |
| Templates | one per server; `serialized_source_guild` uses placeholder integer ids |

## E5. What REST-only can't do (spec US3 scenario 4 and edge cases)

- **Fact**: There is **no REST endpoint that lists who is in a voice channel**. Voice states come
  over the Gateway, or one member at a time (`GET guilds/{g}/voice-states/{user}`). Joining
  voice, `@me` voice states and playing soundboard sounds also need a voice connection.
- **Decision**: Voice tools act on **members the agent names**. "Move everyone from Lobby" works
  only for a member list the agent gives, checked one by one, up to 25 members per call. The
  spec's US3 scenario 4 is reworded to match, and a "move everyone" story is recorded for the
  Gateway milestone (PLAN.md M5). Playing soundboard sounds is out of scope.

## E6. Spec file: new kinds and how each is identified

| Kind (section) | Matched by | Notes |
|---|---|---|
| Server settings (`[guild.settings]`) | singleton | Used only when `manage_settings = true`, which lifts v1-R21. Icon, banner and splash are assets (E7). |
| Automod rules (`[[automod]]`) | id, then name | |
| Forum tags (channel `tags`) | id, then name within the channel | Already in the v1 channel model; now planned too. |
| Onboarding (`[onboarding]`) | singleton; prompts by id, then title; options by id, then title | |
| Welcome screen (`[welcome_screen]`) | singleton | |
| Emoji / stickers / sounds (`[[emoji]]`, `[[stickers]]`, `[[sounds]]`) | id, then name | Assets (E7). |
| Webhooks (`[[webhooks]]`) | id, then (name, channel) | Only *incoming* webhooks. Follower and app webhooks are reported as read-only. No secrets (E8). |
| Invites (`[[invites]]`) | `code`, else a **signature** (channel, max_age, max_uses, temporary, roles) among invites created by the bot | Having the signature means re-planning after apply converges without a code. Ambiguous matches are errors. Expired: reported; recreated only with `recreate = true`. |
| Events (`[[events]]`) | id, then (name, start) | Past or completed events are reported and never recreated. Active events: only description and end time can change. |
| Fixed messages (`[[messages]]`) | id, else bot-authored messages in the channel with **exactly the same** content and embeds | Edit in place needs an `id`. E9 writes it back. Without an id, changing the text posts a new message and the plan warns about it. |

**Shared rules**: v1-R12 matching, unmanaged items reported, and `[prune]` gains `automod`,
`emoji`, `stickers`, `sounds`, `webhooks`, `invites`, `events` and `messages`, all `false` by
default. Kinds that are short-lived or belong to members (bans, member roles, ordinary
messages, threads, polls, voice states, stage instances) can't appear in the file (FR-043).

## E7. Assets (emoji, stickers, sounds, server images)

- **Decision**: An asset is `file = "assets/emoji/logo.png"` (relative to the spec file) or
  `url = "https://…"`.
  - **Export** downloads every asset into `assets/<kind>/<name>.<ext>` next to the spec file
    and writes `file = …`.
  - **Plan** compares `blake3(local bytes)` with `blake3(live bytes fetched from the CDN)`. Any
    difference is a **replace** (delete + create; update for server images). Live bytes are
    fetched only for kinds the spec lists, and cached by hash for the run.
  - **Verify first (unverified)**: that Discord's CDN returns the uploaded bytes unchanged for
    emoji, stickers and sounds. If a kind is re-encoded, compare *decoded pixels* for images
    (`image` crate, behind a feature flag) and duration plus size for sounds. That kind's
    convergence test is the gate.
  - **File reads**: when called through MCP, guildsmith reads local files only under
    `GUILDSMITH_FILE_ROOTS` (empty by default, which means only `url` and inline base64 are
    accepted). The CLI also allows the spec file's own directory.

## E8. No secrets in the spec file (FR-039)

- **Decision**: Webhook `token` and `url` are never exported, stored or diffed.
  - A webhook created by apply appears once in the apply result as `secrets: [{webhook, url}]`.
    That output is marked "secret: copy now". It is never logged or cached, and it is left out
    of `--json` unless `--show-secrets` is passed.
  - Validation rejects a spec that contains anything shaped like a webhook URL or token.

## E9. Writing ids back into the spec file

- **Decision**: `apply --write-ids` (on by default when the CLI is given a file path) and
  `spec_apply(path?)` write the ids of newly created items into the user's file.
  - Writing goes through `toml_edit`, so comments and formatting are kept, and only `id = …`
    keys are added.
  - This replaces v1's "never rewrite the user's file" rule, but only for adding ids. The ids
    are created items' identity, not a separate state file, so constitution III still holds.
- **Rationale**: Fixed messages and invites without a code have no other natural key. Ids also
  make later renames of every kind safe.

## E10. Bulk actions (FR-005)

- **Decision**: One generic `BulkAction<T>` in `guildsmith-app`.
  1. **Preview**: list the targets, or the first 50 plus a total, with per-item problems (above
     the bot's role, owner, self, or too old).
  2. **Confirmed run**: run in batches (bulk ban 200, bulk delete 100, everything else one at a
     time through the rate limiter).
  3. **Progress**: MCP `notifications/progress` when the client sent a `progressToken`; stderr
     for the CLI.
  4. **Cancel**: MCP `notifications/cancelled` or Ctrl-C stops between items.
  5. **Report**: `{succeeded, failed: [{item, reason}], not_attempted}` (SC-005).
- **Message purge**: ids older than 14 days are split out first. They are deleted one by one
  only with `include_old = true`, and the preview shows the extra request count.

## E11. Message search

- **Decision**: `messages_search` uses the guild search endpoint (E3).
  - If the bot lacks the MESSAGE_CONTENT intent (403 or empty content), or the server isn't
    indexed yet (202), the result says so.
  - With `scan = true`, it falls back to scanning up to 500 recent messages per listed channel
    (FR-020's stated limit).
  - Search results are read in `offset` steps. Pagination never relies on how many results came
    back.

## E12. Template import (FR-035/044)

- **Decision**: A pure mapping in `guildsmith-core::template` from `serialized_source_guild` to
  a `GuildSpec`:
  - placeholder ids become names, for roles, overwrite targets and parent categories;
  - roles, categories, channels and settings are carried over;
  - ids are dropped, so names are used when planning against a real server.
  - Fixtures from 3 public templates are snapshot-tested, and each must pass `spec_validate`
    (SC-009).
- **No server creation**: `template_import` always produces a file. Asking to create a server
  returns v1's explanation (FR-035, US7 scenario 4).

## E13. Group sizes and permissions

| Group | Tools | Bot permissions needed (checked by `doctor` and `bot_preflight`) |
|---|---|---|
| default (`core` + dispatcher) | 23 | v1 core |
| `moderation` | 10 | BAN_MEMBERS, KICK_MEMBERS, MODERATE_MEMBERS, MANAGE_GUILD, MANAGE_MESSAGES, MANAGE_ROLES, VIEW_AUDIT_LOG |
| `messages` | 9 | SEND_MESSAGES, MANAGE_MESSAGES, PIN_MESSAGES, ADD_REACTIONS, READ_MESSAGE_HISTORY (+ MESSAGE_CONTENT intent for search) |
| `threads` | 6 | CREATE_PUBLIC/PRIVATE_THREADS, MANAGE_THREADS, SEND_MESSAGES_IN_THREADS, MANAGE_CHANNELS (tags) |
| `polls` | 3 | SEND_POLLS |
| `events` | 5 | CREATE_EVENTS, MANAGE_EVENTS (+ stage/voice permissions per event type) |
| `invites` | 4 | CREATE_INSTANT_INVITE, MANAGE_GUILD, MANAGE_CHANNELS, MANAGE_ROLES (for invite roles) |
| `voice` | 4 | MOVE_MEMBERS, MUTE_MEMBERS, DEAFEN_MEMBERS, MANAGE_CHANNELS, SET_VOICE_CHANNEL_STATUS |
| `automod` | 3 | MANAGE_GUILD (+ MODERATE_MEMBERS for timeout actions) |
| `webhooks` | 5 | MANAGE_WEBHOOKS |
| `expressions` | 4 | CREATE_GUILD_EXPRESSIONS, MANAGE_GUILD_EXPRESSIONS |
| `settings` | 7 | MANAGE_GUILD, MANAGE_ROLES (onboarding), ADMINISTRATOR (COMMUNITY/DISCOVERABLE toggles) |
| `raw` | 1 | whatever the request needs; Discord's error is returned |
| **total** | **84** | |

Every optional group is at most 15 tools (SC-003). Description budget: 1,500 tokens per group
and 15,000 for `all`, measured in CI the same way as v1-R16.

## E-verification. Live checks (T063, T074)

These settle the *(unverified)* items above. Each needs a live test server; the results are
recorded here when run.

| Check | Status | What the code does meanwhile |
|---|---|---|
| **T063**: does `GET webhooks/{id}` return `token` for incoming webhooks when the bot has MANAGE_WEBHOOKS? | **Pending** (needs a live server; not run in this environment) | `webhook_send` uses the in-memory token of a webhook created in this process first, then any token `GET webhooks/{id}` returns. With neither, it fails with an explanation. Both paths are covered by `FakeDiscord` (`set_webhook_tokens_returned`) and `crates/guildsmith-discord/tests/webhooks_path.rs`. |
| **T074**: does Discord's CDN return uploaded emoji, sticker and sound bytes unchanged? | **Pending** (needs a live server; not run in this environment) | Plans compare `blake3` of the local file with `blake3` of the CDN bytes (E7). If a kind turns out to be re-encoded, its convergence test is the gate: open a follow-up to use decoded-pixel comparison (`pixel-compare` feature, off by default) for that kind. |
