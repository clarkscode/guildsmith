---
description: "Task list for 002-extended-toolsets"
---

# Tasks: Extended toolsets — full Discord server coverage

**Input**: Design documents from `/specs/002-extended-toolsets/` ([plan.md](plan.md),
[spec.md](spec.md), [research.md](research.md) E1–E13, [data-model.md](data-model.md),
[contracts/](contracts/), [quickstart.md](quickstart.md))

**Prerequisites**: 001-guildsmith-v1 must be implemented through milestone M3 (workspace,
`core` toolset, policy, `FakeDiscord`, toolset mechanism, budget gate). This feature only adds
to that code.

**Tests**: Included. Constitution principle V requires them (property tests for core,
`FakeDiscord` service tests, recorded HTTP fixtures, MCP protocol tests). Within each story,
write the tests first and confirm they fail before implementing.

**Organization**: Grouped by user story in spec priority order (US1 P1 … US10 P10).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on unfinished tasks)
- **[Story]**: The user story the task belongs to (US1–US10)
- Paths follow the workspace layout in [plan.md](plan.md) (`crates/<crate>/src/…`)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Check the v1 baseline, add dependencies, and record the design decisions.

- [ ] T001 Check the v1 baseline: run `cargo test --workspace` and `cargo xtask budgets` and confirm both pass, that `crates/guildsmith-app/src/fake.rs` exists, and that `GUILDSMITH_TOOLSETS` parsing exists. Record the result and the commit hash in `specs/002-extended-toolsets/checklists/prereqs.md`
- [ ] T002 Add workspace dependencies `base64`, `infer` and optional `image` (feature `pixel-compare`, off by default) in `Cargo.toml`, and wire them into `crates/guildsmith-core/Cargo.toml` and `crates/guildsmith-app/Cargo.toml`. Run `cargo deny check` (MIT/Apache-compatible licenses only)
- [ ] T003 [P] Write ADR-004 "Dispatcher tools instead of dynamic tool lists" (research E1: MCP 2026-07-28 side-effect rule, Codex `mcp_tool_list_changed` off by default, GitHub removing dynamic toolsets on 2026-05-20) in `docs/adr/004-dispatcher-not-dynamic-tool-lists.md`
- [ ] T004 [P] Write ADR-005 "Raw request group: exception to constitution IV" (research E2, plan Complexity Tracking) in `docs/adr/005-raw-group.md`
- [ ] T005 [P] Write ADR-006 "Writing created ids back into the spec file" (research E9: only `id = …` / `code = …` lines, comments kept) in `docs/adr/006-id-writeback.md`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared machinery that every new group needs: the toolset registry, bulk actions,
the ext request path, assets, untrusted wrapping, CLI group commands and budgets.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### Tests (write first)

- [ ] T006 [P] Unit tests for the toolset registry in `crates/guildsmith-app/tests/toolsets.rs`:
  - `GUILDSMITH_TOOLSETS=all` registers every group, and `default` registers only `core`;
  - an unknown name is a startup error that lists the valid names;
  - `GUILDSMITH_TOOLSETS_BLOCK` takes precedence over `GUILDSMITH_TOOLSETS`;
  - no agent-facing API can change `blocked`, the allowlist or read-only mode;
  - an enablement emits an `info` record `{at, toolset, by}`.
- [ ] T007 [P] Property test in `crates/guildsmith-app/tests/bulk_props.rs`. For any targets and any injected per-item failure or cancel point, `|succeeded| + |failed| + |not_attempted| == |targets|` (data-model §2, SC-005). Batches hold at most 200 (bans), 100 (bulk delete) or 1 (others).
- [ ] T008 [P] wiremock tests for the raw/ext request path in `crates/guildsmith-discord/tests/raw_path.rs`:
  - `X-Audit-Log-Reason` is percent-encoded UTF-8, prefixed `guildsmith: `, and cut to 512 characters;
  - `token` fields are removed from responses, and webhook `url` values become `"<redacted>"`;
  - a 429 response is retried by twilight's limiter, not by our code;
  - non-GET methods are never retried on 5xx.
- [ ] T009 [P] Unit tests for assets in `crates/guildsmith-core/tests/asset.rs`. Type detection and size limits per kind:
  - emoji: "256 KiB, JPEG/PNG/GIF/WebP/AVIF";
  - sticker: "512 KiB, PNG/APNG/GIF";
  - sound: "MP3/Ogg, 512 KB, ≤ 5.2 s";
  - message attachment: "20 MiB per file, 25 MiB per request".
- [ ] T010 [P] Unit tests in `crates/guildsmith-app/tests/assets.rs`. Over MCP, files outside `GUILDSMITH_FILE_ROOTS` are refused (and every path is refused when it's empty). The CLI also allows the spec file's own directory.

### Implementation

- [ ] T011 Implement `Toolset` (`core, moderation, messages, threads, polls, events, invites, voice, automod, webhooks, expressions, settings, raw`), `ToolsetState { registered, unlocked, blocked }`, env parsing (`GUILDSMITH_TOOLSETS`, `GUILDSMITH_TOOLSETS_BLOCK`) and enablement logging to stderr in `crates/guildsmith-app/src/toolsets.rs`. Extend v1's existing toolset parsing; don't duplicate it.
- [ ] T012 Implement `BulkAction<T>` and `BulkOutcome<T>` in `crates/guildsmith-app/src/bulk.rs`:
  - the preview lists every target, or the first 50 plus a total, with per-item problems ("role at or above the bot, owner, self, too old, not in voice");
  - batch sizes 200/100/1;
  - a progress callback;
  - cancellation between items;
  - states `Previewed → Running → (Completed | Cancelled | Stopped(error))`.
- [ ] T013 Implement `send_raw(method, path, query, body, reason)` in `crates/guildsmith-discord/src/raw.rs`, using `twilight_http::request::RequestBuilder::raw` with `client.request::<T>()` (the same auth and rate limiter). It percent-encodes the audit reason itself, because twilight's `audit_header` is private, and provides the `redact()` helper for `token` and webhook `url`. Add the `crates/guildsmith-discord/src/ext/mod.rs` module skeleton.
- [ ] T014 [P] Split `crates/guildsmith-app/src/fake.rs` into `crates/guildsmith-app/src/fake/mod.rs`, with a generic per-kind resource store, failure injection by (method, target), and an id generator, so each story can add `fake/<group>.rs`.
- [ ] T015 [P] Implement `Asset` (`file | url | data`), type detection with `infer`, the per-kind limits from T009, and `blake3` hashing of bytes handed in (no I/O) in `crates/guildsmith-core/src/asset.rs`.
- [ ] T016 [P] Implement asset loading in `crates/guildsmith-app/src/assets.rs`: read files under `GUILDSMITH_FILE_ROOTS` (plus the spec file's directory for the CLI), fetch `url` assets, decode `data`.
- [ ] T017 [P] Generalize untrusted-content wrapping to any named user-text field (`shape::untrusted(&[fields])`), and escape closing tags inside content, in `crates/guildsmith-mcp/src/shape.rs` (v1-R19).
- [ ] T018 Register every group the operator chose at startup as normal tools, in a fixed order, in `crates/guildsmith-mcp/src/toolsets.rs`. Override `list_tools` to send a long `ttlMs` with `cacheScope: public`, instead of rmcp's default `ttlMs: 0`. Startup does no network I/O (v1-R17).
- [ ] T019 Add CLI group commands generated from the same registry in `crates/guildsmith/src/cli/groups.rs`: `guildsmith <group> <tool> [--flags]`; `--yes` sets `confirm: true`; without it, a TTY shows the preview and asks, and a non-TTY prints the preview and exits `3` (contracts/cli.md).
- [ ] T020 Extend `bot_preflight`/`doctor` with permission and intent requirements per group (research E13 table, including MESSAGE_CONTENT for search and `PRUNE_REQUIRES_ADMIN` → ADMINISTRATOR) in `crates/guildsmith-app/src/preflight.rs`.
- [ ] T021 Extend the budget gate in `xtask/src/budgets.rs`: each optional group "≤ 15 tools and ≤ 1,500 description tokens"; default "≤ 25 tools and ≤ 3,000 tokens"; `all` "≤ 15,000 description tokens" (contracts/mcp-tools.md).

**Checkpoint**: Groups can be registered by the operator, bulk actions and raw sends work
against fixtures, and the budgets are enforced.

---

## Phase 3: User Story 1 - Full moderation (Priority: P1) 🎯 MVP

**Goal**: The `moderation` group (10 tools) replaces v1's 4 tools: bans (single and bulk),
unban, bans list, timeout, kick, prune, message purge, bulk role assignment, audit log and raid
lockdown.

**Independent Test**: With `GUILDSMITH_TOOLSETS=core,moderation` on a test server, every action
previews first with exact targets or counts, and nothing happens without confirmation
(quickstart §2).

### Tests for User Story 1

- [ ] T022 [P] [US1] wiremock tests for bulk ban in `crates/guildsmith-discord/tests/ext_bans.rs`: `POST guilds/{g}/bulk-ban` with `user_ids` ("at most 200") and `delete_message_seconds` ("≤ 604800"); response `{banned_users, failed_users}`; error `500000` when no one could be banned.
- [ ] T023 [P] [US1] FakeDiscord service tests in `crates/guildsmith-app/tests/moderation.rs`:
  - previews change nothing;
  - the owner and the bot itself are never targeted;
  - members at or above the bot's highest role fail with a reason while the rest continue;
  - 450 bans run as 3 batches;
  - timeout is limited to "≤ 28 days";
  - prune `days` is limited to "1–30";
  - purge splits out messages "older than 14 days" and deletes them one by one only with `include_old = true`;
  - `raid_lockdown` durations are limited to "≤ 24 h".
- [ ] T024 [P] [US1] MCP protocol tests in `crates/guildsmith-mcp/tests/moderation.rs`: all 10 tools are listed with annotations matching contracts/mcp-tools.md (D/W/RO/I); outputs validate against `outputSchema`; a destructive tool without `confirm` returns `{preview: true}`; read-only mode refuses W/D tools before any request.

### Implementation for User Story 1

- [ ] T025 [P] [US1] Implement the bulk ban ext endpoint (request and response serde types, sent through `send_raw`) in `crates/guildsmith-discord/src/ext/bans.rs`.
- [ ] T026 [P] [US1] Define `ModerationPort` in `crates/guildsmith-app/src/ports/moderation.rs`: ban, bulk ban, unban, list bans (`limit` ≤ 1000, `before`/`after`), timeout, kick, prune count, prune (`compute_prune_count = false` for large servers), list messages, bulk delete (2–100 ids), member role add/remove, audit log, incident actions.
- [ ] T027 [US1] Implement `ModerationPort` for twilight in `crates/guildsmith-discord/src/moderation.rs`, using twilight builders where they exist and `ext/bans.rs` for bulk ban.
- [ ] T028 [P] [US1] Implement `ModerationPort` for FakeDiscord in `crates/guildsmith-app/src/fake/moderation.rs`.
- [ ] T029 [US1] Implement the moderation service in `crates/guildsmith-app/src/services/moderation.rs`. The 10 operations use `BulkAction`, and the policy and preflight checks run first. Bulk delete needs MANAGE_MESSAGES; bulk ban needs BAN_MEMBERS **and** MANAGE_GUILD; prune needs ADMINISTRATOR when the server has `PRUNE_REQUIRES_ADMIN`.
- [ ] T030 [US1] Implement the 10 MCP tools (`member_ban`, `ban_remove`, `bans_list`, `member_timeout`, `member_kick`, `members_prune`, `messages_purge`, `roles_bulk_assign`, `audit_log`, `raid_lockdown`) in `crates/guildsmith-mcp/src/tools/moderation.rs`, replacing v1's 4-tool module. Use MCP progress and cancellation notifications for bulk tools.
- [ ] T031 [P] [US1] CLI snapshot tests for `guildsmith moderation ban|purge|prune` (preview output, exit `3` on a non-TTY without `--yes`) in `crates/guildsmith/tests/cli_moderation.rs`.
- [ ] T032 [P] [US1] Write the `guild-moderation` skill (raid response, using previews, bulk ban, purge limits) in `plugins/skills/guild-moderation/SKILL.md`.

**Checkpoint**: The moderation group works on its own (MVP).

---

## Phase 4: User Story 2 - Full messaging, threads and forums (Priority: P2)

**Goal**: The `messages` (9), `threads` (6) and `polls` (3) groups.

**Independent Test**: With those three groups enabled, every request in spec US2 works in
Discord, and user content comes back wrapped (quickstart §3).

### Tests for User Story 2

- [ ] T033 [P] [US2] wiremock tests for guild message search in `crates/guildsmith-discord/tests/ext_search.rs`: `limit` "≤ 25", `offset` "≤ 9975", a `202` with code `110000` means not indexed yet (return `retry_after`), and `messages` is an array of arrays.
- [ ] T034 [P] [US2] FakeDiscord service tests in `crates/guildsmith-app/tests/messages.rs`:
  - only content the bot wrote can be edited (others get flags only);
  - pins are limited to "250 per channel";
  - Components V2 rejects `content`/`embeds`/`poll` and allows "≤ 40 components";
  - mentions default to none (no `@everyone` unless allowed);
  - `scan = true` fallback reads at most 500 messages per channel.
- [ ] T035 [P] [US2] FakeDiscord service tests in `crates/guildsmith-app/tests/threads.rs`: names "1–100"; auto-archive in "60/1440/4320/10080"; forum tags "≤ 20 per forum, ≤ 5 per post"; an unknown tag fails with the valid tags listed; a tag-required forum refuses untagged posts.
- [ ] T036 [P] [US2] FakeDiscord service tests in `crates/guildsmith-app/tests/polls.rs`: answers "≤ 10", question "≤ 300 chars", answers "≤ 55", duration "≤ 768 h"; only the bot's own polls can be ended.
- [ ] T037 [P] [US2] MCP protocol tests for the 18 tools in `crates/guildsmith-mcp/tests/messages_threads_polls.rs`: annotations, schemas, wrapping of message content, thread names and poll answers.

### Implementation for User Story 2

- [ ] T038 [P] [US2] Implement the guild message search ext endpoint in `crates/guildsmith-discord/src/ext/search.rs`.
- [ ] T039 [P] [US2] Define `MessagesPort`, `ThreadsPort` and `PollsPort` in `crates/guildsmith-app/src/ports/{messages,threads,polls}.rs`. Use the new `messages/pins` routes, not the deprecated `/pins` ones.
- [ ] T040 [US2] Implement the three ports for twilight (`pins`, `create_pin`, `delete_pin`, `reactions`, `crosspost_message`, `create_thread*`, `update_thread`, `*_archived_threads`, `thread_members`, `create_message().poll()`, `end_poll`, `get_answer_voters`, `ext/search.rs`) in `crates/guildsmith-discord/src/{messages,threads,polls}.rs`.
- [ ] T041 [P] [US2] Implement the three ports for FakeDiscord in `crates/guildsmith-app/src/fake/{messages,threads,polls}.rs`.
- [ ] T042 [US2] Implement the services in `crates/guildsmith-app/src/services/{messages,threads,polls}.rs`. That covers the validation from T034–T036, the search fallback (research E11), and attachments through `assets.rs`.
- [ ] T043 [US2] Implement the MCP tools in `crates/guildsmith-mcp/src/tools/{messages,threads,polls}.rs`:
  - messages (9): `message_compose`, `message_edit`, `message_delete`, `message_pin`, `pins_list`, `message_crosspost`, `reactions`, `reactions_list`, `messages_search`;
  - threads (6): `thread_create`, `thread_update`, `thread_delete`, `threads_list`, `thread_members`, `forum_tags`;
  - polls (3): `poll_create`, `poll_results`, `poll_end`.
- [ ] T044 [P] [US2] Write the `guild-community` skill (messages, threads, forums, polls) in `plugins/skills/guild-community/SKILL.md`.

**Checkpoint**: US1 and US2 work independently.

---

## Phase 5: User Story 3 - Events, invites, voice and stage (Priority: P3)

**Goal**: The `events` (5), `invites` (4) and `voice` (4) groups.

**Independent Test**: Spec US3 scenarios on a test server. Voice actions apply to named members,
and members not in voice are reported (research E5).

### Tests for User Story 3

- [ ] T045 [P] [US3] wiremock tests in `crates/guildsmith-discord/tests/ext_events_invites_voice.rs`:
  - event `recurrence_rule` bodies follow the limits: "no count/end, one by-* entry, interval only for weekly";
  - invite `role_ids` and the target-user endpoints (`/invites/{code}/target-users`, bulk add/delete "max 1000");
  - `PUT channels/{c}/voice-status` with `status` "≤ 500 chars".
- [ ] T046 [P] [US3] FakeDiscord service tests in `crates/guildsmith-app/tests/events_invites_voice.rs`:
  - "≤ 100 scheduled or active" events;
  - external events need a location and an end;
  - times without a zone are UTC and shown as "UTC";
  - invites `max_age` "≤ 604800", `max_uses` "≤ 100", invite roles below the bot only;
  - voice mute/deafen of a member not in voice is reported;
  - `voice_members` takes "≤ 25" members;
  - stage topic "1–120".
- [ ] T047 [P] [US3] MCP protocol tests for the 13 tools in `crates/guildsmith-mcp/tests/events_invites_voice.rs`.

### Implementation for User Story 3

- [ ] T048 [P] [US3] Implement the ext endpoints in `crates/guildsmith-discord/src/ext/events.rs` (create/update bodies with `recurrence_rule`), `crates/guildsmith-discord/src/ext/invites.rs` (`role_ids`, target users) and `crates/guildsmith-discord/src/ext/voice.rs` (voice channel status).
- [ ] T049 [P] [US3] Define `EventsPort`, `InvitesPort` and `VoicePort` in `crates/guildsmith-app/src/ports/{events,invites,voice}.rs`.
- [ ] T050 [US3] Implement the three ports for twilight in `crates/guildsmith-discord/src/{events,invites,voice}.rs`: `update_guild_member().channel_id()/.mute()/.deaf()`, `user_voice_state`, stage instance CRUD, `create_invite`, `guild_invites`, `delete_invite`, and the ext modules.
- [ ] T051 [P] [US3] Implement the three ports for FakeDiscord in `crates/guildsmith-app/src/fake/{events,invites,voice}.rs`.
- [ ] T052 [US3] Implement the services in `crates/guildsmith-app/src/services/{events,invites,voice}.rs`, with the validation from T046 and `BulkAction` for `voice_members`.
- [ ] T053 [US3] Implement the MCP tools in `crates/guildsmith-mcp/src/tools/{events,invites,voice}.rs`:
  - events: `event_create`, `event_update`, `event_delete`, `events_list`, `event_users`;
  - invites: `invite_create`, `invites_list`, `invite_revoke`, `invite_target_users`;
  - voice: `voice_members`, `voice_state`, `stage`, `voice_channel_status`.
- [ ] T054 [P] [US3] Add an events/invites/voice section to `plugins/skills/guild-community/SKILL.md`, including the REST-only voice limitation.

**Checkpoint**: US1–US3 work independently.

---

## Phase 6: User Story 4 - Auto-moderation rules (Priority: P4)

**Goal**: The `automod` group (3 tools), with every trigger, action and exemption type.

**Independent Test**: Create, list, update and delete every rule type on a test server; a
matching message triggers the rule.

### Tests for User Story 4

- [ ] T055 [P] [US4] wiremock tests in `crates/guildsmith-discord/tests/ext_automod.rs`: our own create/update bodies leave out unset fields (twilight 0.17.1's update sends `null` for them); `mention_raid_protection_enabled`, the member-profile trigger and the block-member-interaction action serialize correctly.
- [ ] T056 [P] [US4] Validation tests in `crates/guildsmith-app/tests/automod.rs`:
  - "keyword rules ≤ 6, other types 1 each";
  - "keywords ≤ 1000 × 60 chars; regex ≤ 10 × 260; allow list ≤ 100 (1000 for presets)";
  - "mention limit ≤ 50";
  - "exempt roles ≤ 20, channels ≤ 50";
  - "timeout action ≤ 2419200 s", only on keyword and mention-spam rules;
  - disabling a rule needs `confirm`.

### Implementation for User Story 4

- [ ] T057 [P] [US4] Implement the automod ext endpoints (create and update bodies) in `crates/guildsmith-discord/src/ext/automod.rs`.
- [ ] T058 [P] [US4] Define `AutomodPort` in `crates/guildsmith-app/src/ports/automod.rs`, implement it for twilight in `crates/guildsmith-discord/src/automod.rs` (list/get/delete through twilight, create/update through ext), and for FakeDiscord in `crates/guildsmith-app/src/fake/automod.rs`.
- [ ] T059 [US4] Implement the automod service (trigger/action model from data-model §4 `AutomodRuleSpec`, plus the validation from T056) in `crates/guildsmith-app/src/services/automod.rs`.
- [ ] T060 [US4] Implement the MCP tools `automod_rules_list`, `automod_rule_upsert` and `automod_rule_delete` in `crates/guildsmith-mcp/src/tools/automod.rs`, with protocol tests in `crates/guildsmith-mcp/tests/automod.rs`.

**Checkpoint**: US1–US4 work independently.

---

## Phase 7: User Story 5 - Webhooks (Priority: P5)

**Goal**: The `webhooks` group (5 tools). Webhook URLs and tokens never leak.

**Independent Test**: Create, list, rename, move, send through and delete a webhook. The URL
appears only in the create response (SC-006).

### Tests for User Story 5

- [ ] T061 [P] [US5] Secret-leak tests in `crates/guildsmith-app/tests/webhook_secrets.rs`. With seeded webhook tokens, every list output, log line (captured at `trace`), error and cached value contains no token or URL; only the `webhook_create` result has `secrets: {url}`.
- [ ] T062 [P] [US5] FakeDiscord service tests in `crates/guildsmith-app/tests/webhooks.rs`: names "1–80, no 'clyde' or 'discord'" (case-insensitive); "≤ 15 per channel, ≤ 1000 per server"; follower and app webhooks are read-only; deleting needs `confirm`.

### Implementation for User Story 5

- [ ] T063 [US5] Check against a live test server whether `GET webhooks/{id}` returns `token` for incoming webhooks when called with MANAGE_WEBHOOKS. Record the answer in `specs/002-extended-toolsets/research.md` §E-verification (it resolves the *unverified* note in contracts/mcp-tools.md).
- [ ] T064 [P] [US5] Implement an in-memory webhook token store (never serialized, redacting `Debug`, cleared on exit) in `crates/guildsmith-app/src/secrets.rs`.
- [ ] T065 [P] [US5] Define `WebhooksPort` in `crates/guildsmith-app/src/ports/webhooks.rs`, implement it for twilight in `crates/guildsmith-discord/src/webhooks.rs` (`create_webhook`, `channel_webhooks`, `guild_webhooks`, `update_webhook`, `delete_webhook`, `execute_webhook` with `wait`/`thread_id`), and for FakeDiscord in `crates/guildsmith-app/src/fake/webhooks.rs`.
- [ ] T066 [US5] Implement the webhooks service in `crates/guildsmith-app/src/services/webhooks.rs`. Token lookup follows T063's result, falling back to the T064 store.
- [ ] T067 [US5] Implement the MCP tools `webhook_create`, `webhooks_list`, `webhook_update`, `webhook_delete` and `webhook_send` in `crates/guildsmith-mcp/src/tools/webhooks.rs`, with protocol tests in `crates/guildsmith-mcp/tests/webhooks.rs`.

**Checkpoint**: US1–US5 work independently.

---

## Phase 8: User Story 6 - Emoji, stickers and soundboard (Priority: P6)

**Goal**: The `expressions` group (4 tools), with approximate slot counts.

**Independent Test**: Upload, rename, list and delete emoji, stickers and sounds; the list shows
used and remaining slots.

### Tests for User Story 6

- [ ] T068 [P] [US6] wiremock tests for soundboard CRUD (`guilds/{g}/soundboard-sounds[/{id}]`, list returns `{items}`, `sound` as an MP3/Ogg data URI) in `crates/guildsmith-discord/tests/ext_soundboard.rs`.
- [ ] T069 [P] [US6] FakeDiscord service tests in `crates/guildsmith-app/tests/expressions.rs`:
  - emoji names "2–32, `[A-Za-z0-9_]`";
  - sticker name "2–30", description "empty or 2–100", tags "≤ 200";
  - sound name "2–32", volume "0–1";
  - Lottie stickers are refused unless the server is verified or partnered;
  - oversized files are refused before upload;
  - slots use the approximate table "emoji 50/100/150/250 (static and animated each), stickers 5/15/30/60, sounds 8/24/36/48", labelled "approximately".

### Implementation for User Story 6

- [ ] T070 [P] [US6] Implement the soundboard ext endpoints and our own `SoundboardSound` model in `crates/guildsmith-discord/src/ext/soundboard.rs`.
- [ ] T071 [P] [US6] Define `ExpressionsPort` in `crates/guildsmith-app/src/ports/expressions.rs`, implement it for twilight in `crates/guildsmith-discord/src/expressions.rs` (emoji through data URIs built with `base64`, stickers through `create_guild_sticker` multipart, sounds through ext), and for FakeDiscord in `crates/guildsmith-app/src/fake/expressions.rs`.
- [ ] T072 [US6] Implement the expressions service (validation from T069, slot computation from the premium tier) in `crates/guildsmith-app/src/services/expressions.rs`.
- [ ] T073 [US6] Implement the MCP tools `expressions_list`, `expression_create`, `expression_update` and `expression_delete` in `crates/guildsmith-mcp/src/tools/expressions.rs`, with protocol tests in `crates/guildsmith-mcp/tests/expressions.rs`.
- [ ] T074 [US6] On a live test server, upload an emoji, a sticker and a sound, download each from the CDN, and compare the bytes (research E7). Record the result per kind in `specs/002-extended-toolsets/research.md` §E-verification. If any kind is re-encoded, open a follow-up task to turn on `pixel-compare` for it in US9.

**Checkpoint**: US1–US6 work independently.

---

## Phase 9: User Story 7 - Server settings, onboarding and templates (Priority: P7)

**Goal**: The `settings` group (7 tools), with template import into a spec file.

**Independent Test**: Change settings, onboarding and the welcome screen; create, sync and import
a template; the imported file passes validation (SC-009).

### Tests for User Story 7

- [ ] T075 [P] [US7] Snapshot tests for template import in `crates/guildsmith-core/tests/template.rs`. Use 3 recorded public-template fixtures in `crates/guildsmith-core/tests/fixtures/templates/*.json`. Each imported `GuildSpec` passes `spec_validate`, placeholder ids become names, and the output has no ids.
- [ ] T076 [P] [US7] FakeDiscord service tests in `crates/guildsmith-app/tests/settings.rs`:
  - previews show old → new;
  - `afk_timeout` is one of "60/300/900/1800/3600";
  - COMMUNITY/DISCOVERABLE toggles need ADMINISTRATOR;
  - an enabled onboarding needs "≥ 7 default channels with ≥ 5 that @everyone can send in", computed with v1's explain function;
  - one template per server;
  - asking to create a server returns the "bots can't create servers" explanation.

### Implementation for User Story 7

- [ ] T077 [P] [US7] Implement the pure `serialized_source_guild → GuildSpec` mapping in `crates/guildsmith-core/src/template.rs` (research E12).
- [ ] T078 [P] [US7] Implement the guild ext endpoint for `description` and `safety_alerts_channel_id` in `crates/guildsmith-discord/src/ext/guild.rs`.
- [ ] T079 [P] [US7] Define `SettingsPort` in `crates/guildsmith-app/src/ports/settings.rs`, implement it for twilight in `crates/guildsmith-discord/src/settings.rs` (`update_guild`, `update_guild_onboarding`, `update_guild_welcome_screen`, template CRUD, `get_template`, integrations, widget, vanity), and for FakeDiscord in `crates/guildsmith-app/src/fake/settings.rs`.
- [ ] T080 [US7] Implement the settings service in `crates/guildsmith-app/src/services/settings.rs`, using the `SettingsSpec`, `OnboardingSpec` and `WelcomeScreenSpec` field rules from data-model §4.
- [ ] T081 [US7] Implement the MCP tools `settings_get`, `settings_update`, `onboarding_update`, `welcome_screen_update`, `templates`, `template_import` and `integration_remove` in `crates/guildsmith-mcp/src/tools/settings.rs`, with protocol tests in `crates/guildsmith-mcp/tests/settings.rs`.

**Checkpoint**: Every live group (US1–US7) works with operator-registered groups.

---

## Phase 10: User Story 8 - Turn on the tools you need (Priority: P8)

**Goal**: The `toolsets` and `tool_call` dispatcher tools join the default set (21 → 23), so the
agent can enable any group that isn't blocked during a session.

**Independent Test**: Quickstart §1. Enable `polls` mid-session, use it via `tool_call`, see
that `tools/list` is unchanged, and see a refusal for a blocked group.

### Tests for User Story 8

- [ ] T082 [P] [US8] MCP protocol tests in `crates/guildsmith-mcp/tests/dispatch.rs`:
  - the default `tools/list` has exactly 23 tools;
  - `toolsets(list)` reports every group with `tools`, `approx_tokens`, `required_permissions`, `registered`, `unlocked` and `blocked`;
  - `toolsets(describe)` returns the group's tool definitions;
  - `toolsets(enable)` on a blocked group is refused;
  - `tool_call` to a locked group is refused;
  - `tool_call` applies the same policy as a direct call (preview without `confirm`, read-only refusal);
  - `tools/list` is the same before and after `enable`.

### Implementation for User Story 8

- [ ] T083 [US8] Implement `toolsets` (`action = list | describe | enable`) and `tool_call` (`tool`, `arguments`) in `crates/guildsmith-mcp/src/dispatch.rs`. `tool_call` routes to the same handler as a direct call and returns its result unchanged.
- [ ] T084 [US8] Report the tool count and approximate tokens of the registered groups at startup (stderr) and in `doctor` (FR-010), in `crates/guildsmith/src/main.rs` and `crates/guildsmith/src/cli/doctor.rs`.
- [ ] T085 [P] [US8] Add the `guildsmith toolsets` CLI command in `crates/guildsmith/src/cli/toolsets.rs`.
- [ ] T086 [P] [US8] Update the default budget fixture to 23 tools in `xtask/src/budgets.rs`, and update `docs/tools.md` with the dispatcher, including the approval trade-off (research E1).

**Checkpoint**: Starting with only the default tools, the agent can use every group that isn't
blocked.

---

## Phase 11: User Story 9 - Manage the whole server as code (Priority: P9)

**Goal**: Spec file support for settings, automod, forum tags, onboarding, the welcome screen,
emoji, stickers, sounds, webhooks, invites, events and fixed messages, with convergence (SC-010)
and no secrets (SC-011).

**Independent Test**: Quickstart §4. Export `$LAB`, apply it to `$EMPTY`, and both plans show no
changes.

**Depends on**: US4 (automod), US5 (webhooks), US6 (expressions, plus T074's result), US7
(settings), and US3 (events and invites) for the matching ports. Each kind below can ship once
its port exists.

### Tests for User Story 9

- [ ] T087 [P] [US9] Property tests in `crates/guildsmith-core/tests/converge_kinds.rs`. For each new kind, `diff(spec, apply_fake(spec, s)).ops.is_empty()`. Invites without a `code` converge by the signature "(channel, max_age, max_uses, temporary, sorted(roles))", and messages without an `id` converge by exact "(content, embeds)".
- [ ] T088 [P] [US9] Validation tests in `crates/guildsmith-core/tests/validate_kinds.rs`:
  - a value shaped like a webhook URL or token anywhere in the file is an error (E8);
  - sections for bans, members, member roles, threads, polls, voice states, stage instances or ordinary messages are unknown keys (FR-043);
  - a local datetime without an offset is an error;
  - `components_v2` with `content`/`embeds` is an error;
  - an external event without an `end` is an error;
  - unresolved `#channel`/`@Role` references are errors.
- [ ] T089 [P] [US9] Ordering tests in `crates/guildsmith-core/tests/order_kinds.rs` following data-model §5: v1 structure → settings → expressions → webhooks → automod → onboarding/welcome → events → invites → fixed messages/pins → deletes in reverse dependency order → `WriteIds`.
- [ ] T090 [P] [US9] Write-back tests in `crates/guildsmith-app/tests/writeback.rs`: only `id = "…"` / `code = "…"` lines are added, comments and formatting are kept byte for byte elsewhere, and a second apply writes nothing.

### Implementation for User Story 9

- [ ] T091 [P] [US9] Add spec types for settings (`[guild.settings]`, used only when `manage_settings = true`) in `crates/guildsmith-core/src/spec/settings.rs`.
- [ ] T092 [P] [US9] Add spec types for `[[automod]]` in `crates/guildsmith-core/src/spec/automod.rs`.
- [ ] T093 [P] [US9] Add spec types for `[onboarding]` and `[welcome_screen]` in `crates/guildsmith-core/src/spec/{onboarding,welcome}.rs`.
- [ ] T094 [P] [US9] Add spec types for `[[emoji]]`, `[[stickers]]` and `[[sounds]]` (using `Asset`) in `crates/guildsmith-core/src/spec/expressions.rs`.
- [ ] T095 [P] [US9] Add spec types for `[[webhooks]]` (no token or url fields), `[[invites]]` (`recreate: bool = false`), `[[events]]` and `[[messages]]` in `crates/guildsmith-core/src/spec/{webhooks,invites,events,messages}.rs`.
- [ ] T096 [US9] Extend `PruneSpec` with `automod, emoji, stickers, sounds, webhooks, invites, events, messages` (all `false` by default) and the new top-level `GuildSpec` fields in `crates/guildsmith-core/src/spec/mod.rs`. Regenerate `schema/guild.schema.json` with `cargo xtask schema`.
- [ ] T097 [US9] Implement the diff for each kind and the new `Op` variants with their risk levels (data-model §5) in `crates/guildsmith-core/src/planner/kinds/{settings,automod,onboarding,expressions,webhooks,invites,events,messages}.rs`, and the ordering in `crates/guildsmith-core/src/planner/order.rs`. Rules:
  - past or completed events are reported and never recreated; active events change only description and end;
  - expired invites are recreated only with `recreate = true`;
  - fixed messages are edited only if the bot posted them.
- [ ] T098 [P] [US9] Implement CDN asset download for plan comparison in `crates/guildsmith-discord/src/cdn.rs`: blake3 of live bytes, cached by hash for the run, fetched only for kinds the spec lists. Use pixel comparison for any kind T074 found re-encoded.
- [ ] T099 [US9] Extend export for every kind in `crates/guildsmith-app/src/services/export.rs`: assets are written to `assets/<kind>/<name>.<ext>`, and webhook tokens and URLs are never exported.
- [ ] T100 [US9] Extend apply in `crates/guildsmith-app/src/services/apply.rs`: run the new ops through each group's port; collect `secrets: [{webhook, url}]` (never logged or cached); collect `written_ids`.
- [ ] T101 [US9] Implement id write-back with `toml_edit` in `crates/guildsmith-app/src/writeback.rs`. Add the CLI flags `--write-ids`/`--no-write-ids` (default on with a file path) and `--show-secrets` in `crates/guildsmith/src/cli/apply.rs`, and `spec_apply(path?)` plus the `secrets` and `written_ids` result fields in `crates/guildsmith-mcp/src/tools/spec.rs`.
- [ ] T102 [P] [US9] Add `examples/full-server/guild.toml` with `examples/full-server/assets/` (one item of every kind), and update `docs/spec-reference.md` with every new section from contracts/spec-format.md.
- [ ] T103 [P] [US9] Write the `guild-server-as-code` skill (export with assets, ids written back, secrets shown once, expiring items) in `plugins/skills/guild-server-as-code/SKILL.md`.

**Checkpoint**: The whole server round-trips through the spec file.

---

## Phase 12: User Story 10 - Raw access for anything else (Priority: P10)

**Goal**: The `raw` group: `discord_request`, scoped to allowed servers, redacted and wrapped.

**Independent Test**: Quickstart §5. A read works; a write previews without confirmation; read
-only mode, servers outside the allowlist and token routes are refused (SC-012).

### Tests for User Story 10

- [ ] T104 [P] [US10] Classifier tests in `crates/guildsmith-core/tests/raw_classifier.rs`, covering every row of research E2:
  - `guilds/{id}/…` → `Guild`; `channels/{id}/…` and `stage-instances/{channel}` → `Channel`; `webhooks/{id}` (no token) → `Webhook`; `invites/{code}/…` → `Invite`; `users/@me` GET → `SelfUser`;
  - `users/@me/guilds/{id}` DELETE is a destructive `Guild`;
  - `Refused`: `webhooks/{id}/{token}…`, `applications/…/commands`, `oauth2/…`, `gateway…`, `POST guilds`, and anything that can't be resolved.
- [ ] T105 [P] [US10] Service tests in `crates/guildsmith-app/tests/raw.rs`:
  - GET runs without `confirm`;
  - non-GET previews `{method, path, query, body}`;
  - read-only mode refuses;
  - a channel, webhook or invite in a server outside the allowlist is refused after lookup;
  - a body containing `Authorization` or a token is refused;
  - a confirmed write clears the snapshot cache and carries the audit reason;
  - responses over 50,000 characters are cut and marked.

### Implementation for User Story 10

- [ ] T106 [US10] Gate: before merging this phase, either run `/speckit-constitution` to amend principle IV (1.0.0 → 1.1.0, exception for the opt-in `raw` group) or record the accepted exception in `docs/adr/005-raw-group.md` with reviewer sign-off.
- [ ] T107 [P] [US10] Implement the pure `classify(path, method) -> RawScope` in `crates/guildsmith-core/src/raw.rs`.
- [ ] T108 [US10] Implement the raw service in `crates/guildsmith-app/src/services/raw.rs`: resolve lookups to a server id, apply policy, call `send_raw`, redact, cut to size, clear the cache on writes.
- [ ] T109 [US10] Implement the MCP tool `discord_request` (description points to the dedicated tools first, FR-036d; the whole response is wrapped as untrusted) in `crates/guildsmith-mcp/src/tools/raw.rs`, with protocol tests in `crates/guildsmith-mcp/tests/raw.rs`.
- [ ] T110 [P] [US10] Add the CLI `guildsmith raw <METHOD> <path>` with `{guild}` replaced by the resolved `-g` id, plus `--body`, `--query` and `--yes`, in `crates/guildsmith/src/cli/raw.rs`.
- [ ] T111 [P] [US10] Write the `guild-raw` skill (when to use raw, why dedicated tools come first, redaction) in `plugins/skills/guild-raw/SKILL.md`.

**Checkpoint**: Every user story works.

---

## Phase 13: Polish & Cross-Cutting Concerns

- [ ] T112 [P] Extend the v1 `secret_leak` test to every new group (bot token, webhook tokens, URLs) in `crates/guildsmith/tests/secret_leak.rs`.
- [ ] T113 [P] Add opt-in live tests for each user story (`GUILDSMITH_LIVE_TEST=1`) in `tests/live/{moderation,community,events,automod,webhooks,expressions,settings,server_as_code,raw}.rs`, and add them to `.github/workflows/nightly-live.yml`.
- [ ] T114 [P] Update `docs/tools.md` with every group (tools, annotations, permissions from research E13).
- [ ] T115 Run `cargo xtask sync-plugins` to copy the new skills into `plugins/claude-code/skills/` and `plugins/codex/skills/`, and make sure the CI drift check passes.
- [ ] T116 Check the budgets and the binary size with default features (< 10 MB stripped) and with `all` (cold start to `tools/list` under 50 ms with 84 tools), using `cargo xtask budgets`.
- [ ] T117 Measure the scripted 20-task agent run (SC-007: at least 2 tasks per user story, no failed tool choice) and the 50-account ban timing (SC-008: under 2 minutes). Record the results in `specs/002-extended-toolsets/checklists/acceptance.md`.
- [ ] T118 Run every scenario in `specs/002-extended-toolsets/quickstart.md` §0–§5 and record the results in `specs/002-extended-toolsets/checklists/acceptance.md`.

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (1)** needs v1 through M3 (T001 checks this).
- **Foundational (2)** depends on Setup and **blocks every story**.
- **US1–US7 (3–9)** depend only on Foundational and are independent of each other. Groups can
  be registered by the operator (`GUILDSMITH_TOOLSETS`), so none of them needs US8.
- **US8 (10)** depends on Foundational. It is more useful once some groups exist, but it can be
  built at any time after Phase 2. The plan suggests building it early.
- **US9 (11)** depends on the port of each kind it covers: US3 (events, invites), US4, US5, US6
  (plus T074's result) and US7. Each kind's tasks can start as soon as that port exists.
- **US10 (12)** depends on Foundational (T013) and the T106 gate.
- **Polish (13)** comes after the chosen stories.

### Story dependency graph

```text
Setup → Foundational ─┬─ US1 (moderation) ← MVP
                      ├─ US2 (messages/threads/polls)
                      ├─ US3 (events/invites/voice) ──┐
                      ├─ US4 (automod) ───────────────┤
                      ├─ US5 (webhooks) ──────────────┼─→ US9 (server as code, per kind)
                      ├─ US6 (expressions, T074) ─────┤
                      ├─ US7 (settings/templates) ────┘
                      ├─ US8 (dispatcher)
                      └─ US10 (raw; needs the T106 gate)
                                                      → Polish
```

### Within each story

Tests first, and they must fail before implementation. Then ext endpoints and ports (parallel),
then the adapter and fake implementations, then the service, then MCP tools, then the CLI and
skill.

---

## Parallel Examples

### User Story 1

```bash
# Tests together:
Task: "T022 wiremock bulk-ban tests in crates/guildsmith-discord/tests/ext_bans.rs"
Task: "T023 moderation service tests in crates/guildsmith-app/tests/moderation.rs"
Task: "T024 moderation protocol tests in crates/guildsmith-mcp/tests/moderation.rs"
# Then together:
Task: "T025 ext/bans.rs"   Task: "T026 ports/moderation.rs"   Task: "T028 fake/moderation.rs"
```

### User Story 2

```bash
Task: "T033 ext_search.rs"  Task: "T034 messages.rs tests"  Task: "T035 threads.rs tests"  Task: "T036 polls.rs tests"  Task: "T037 protocol tests"
Task: "T038 ext/search.rs"  Task: "T039 ports/{messages,threads,polls}.rs"  Task: "T041 fake/{messages,threads,polls}.rs"
```

### User Story 9 (spec types are independent files)

```bash
Task: "T091 spec/settings.rs"  Task: "T092 spec/automod.rs"  Task: "T093 spec/{onboarding,welcome}.rs"
Task: "T094 spec/expressions.rs"  Task: "T095 spec/{webhooks,invites,events,messages}.rs"
```

### Across stories (after Phase 2, with several developers or agents)

US1, US2, US3, US4, US5, US6, US7, US8 and US10 (after T106) can run at the same time. They
touch different files, except the shared registry in `crates/guildsmith-mcp/src/toolsets.rs`.
Each story adds one line there, so merge those one at a time.

---

## Implementation Strategy

### MVP (User Story 1 only)

1. Phase 1 Setup, then Phase 2 Foundational.
2. Phase 3 (US1 moderation).
3. **Stop and check**: quickstart §2 with `GUILDSMITH_TOOLSETS=core,moderation`.
4. Release. The moderation group alone covers the most urgent gap against other Discord MCP
   servers.

### Incremental delivery

1. Then US8 (dispatcher), so the agent can enable groups itself before more groups arrive.
2. Then US2 → US3 → US4 → US5 → US6 → US7, each released as its group is finished.
3. Then US9, one kind per PR, each after its group.
4. Then US10, once the T106 gate passes.

### Task counts

| Phase | Tasks |
|---|---|
| Setup | 5 (T001–T005) |
| Foundational | 16 (T006–T021) |
| US1 Moderation | 11 (T022–T032) |
| US2 Messages/threads/polls | 12 (T033–T044) |
| US3 Events/invites/voice | 10 (T045–T054) |
| US4 Automod | 6 (T055–T060) |
| US5 Webhooks | 7 (T061–T067) |
| US6 Expressions | 7 (T068–T074) |
| US7 Settings/templates | 7 (T075–T081) |
| US8 Dispatcher | 5 (T082–T086) |
| US9 Server as code | 17 (T087–T103) |
| US10 Raw | 8 (T104–T111) |
| Polish | 7 (T112–T118) |
| **Total** | **118** |

## Notes

- [P] = different files and no unfinished dependencies. [USn] maps a task to a spec story.
- Constraints from data-model.md and research E4 are quoted in tasks; don't loosen them during
  implementation.
- Commit after each task or logical group, with a test for every bug fix (constitution V).
- `T063` and `T074` are live checks that settle *unverified* research items. Record their results
  before the tasks that depend on them.
