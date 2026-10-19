# Implementation Plan: Extended toolsets — full Discord server coverage

**Branch**: `002-extended-toolsets` | **Date**: 2026-10-19 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/002-extended-toolsets/spec.md`. Builds on
`/specs/001-guildsmith-v1/` (plan, research `v1-R*`, data model, contracts).

## Summary

This feature covers everything a REST bot can do in Discord, in three layers:

1. **Eleven opt-in tool groups** with 60 dedicated tools in all (moderation, messages, threads,
   polls, events, invites, voice, automod, webhooks, expressions, settings).
2. **A `raw` group** with one tool, `discord_request`. It goes through the same auth, rate
   limiter and policy as everything else (E2).
3. **Server as code for all lasting configuration**: settings, automod, forum tags, onboarding,
   the welcome screen, emoji/stickers/sounds, webhooks, invites, events and fixed messages
   (E6–E9).

**The agent turns on groups through a dispatcher, not by changing the tool list** (E1). Two
default tools, `toolsets` and `tool_call`, unlock and run any group in the middle of a session.
This works in every client and follows MCP 2026-07-28. The default set grows from 21 to 23
tools, which is still within the constitution's budget.

**Endpoints twilight 0.17.1 lacks** (bulk ban, soundboard, voice status, search, event
recurrence, the full automod fields, guild description, invite roles) are added in our adapter
through twilight's own `RequestBuilder::raw`, so there's still a single HTTP stack (E3).

## Technical Context

**Language/Version**: Rust, edition 2024. Built with stable 1.99; MSRV 1.97 (v1-R1).

**Primary Dependencies**: The same as v1 (twilight 0.17.1 with `RequestBuilder::raw` for the
gaps, rmcp 3.5.x, tokio, clap, serde, toml/toml_edit, schemars, blake3). New:
- `base64`, for data URIs.
- `infer`, to detect asset file types.
- `image`, **optional** (feature `pixel-compare`), only if the CDN check in E7 shows re-encoding.

**Storage**: Spec files plus an `assets/` folder next to them (E7). Ids are written back into the
spec file (E9). Process memory holds tool group state and webhook tokens created in this process
(never stored on disk).

**Testing**: The same as v1, plus:
- `FakeDiscord` extended to every kind, with property tests for convergence of each kind.
- Tests for the raw path classifier.
- A property test for the `BulkOutcome` count invariant.
- wiremock fixtures for each ext endpoint.
- Snapshot tests for 3 public-template imports.
- Opt-in live tests for each user story.

**Target Platform**: Unchanged (5 targets; Claude Code, Codex and generic MCP clients).

**Project Type**: CLI + MCP server on top of library crates (unchanged).

**Performance Goals**:
- `tools/list` under 50 ms even with `all` (84 tools, all static).
- Bulk actions are limited only by Discord's rate limits; progress is reported.

**Constraints**:
- Default: ≤ 25 tools and ≤ 3k tokens of descriptions.
- Each group: ≤ 15 tools and ≤ 1.5k tokens. `all`: ≤ 15k tokens.
- Binary still under 10 MB with default features.
- No secrets in spec files, logs or lists.
- REST only: no voice presence listing and no soundboard playback (E5).

**Scale/Scope**: 12 new groups, 61 new tools plus 2 dispatcher tools (84 total). 10 new spec-file
kinds. 8 ext endpoints.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Pre-research | Post-design |
|---|---|---|
| **I. Safe by Default** | ✅ Every new tool goes through `guildsmith-app::policy`. | ✅ The dispatcher applies the same policy as a direct call. Raw writes preview, respect read-only mode and carry audit reasons. Bulk actions preview. Webhook URLs and tokens never reach files, logs or lists (E8). User content is wrapped everywhere, raw responses included. The agent can't change the allowlist, read-only mode or the block list (data-model §1). |
| **II. Pure Core, Thin Adapters** | ✅ | ✅ New pure code in core: spec kinds, ops, the raw path classifier and template mapping. Asset **bytes** are loaded by `app` (local files) and the adapter (CDN), then handed to core. Ext endpoints and twilight types stay in `guildsmith-discord`. |
| **III. Plans Converge** | ✅ | ✅ Every kind has a convergence property test. Invites without a code and messages without an id converge by signature or content match (E6). Writing ids back into the spec file (E9) is not a state file. |
| **IV. Lean Agent Interface** | ⚠️ The raw group returns Discord JSON. | ⚠️ Default 23 ≤ 25 ✅; group and `all` budgets ✅; annotations ✅. **Exception**: `discord_request` returns Discord's JSON (with tokens removed, size-limited, wrapped). Justified below. |
| **V. Tested Without the Network** | ✅ | ✅ Quickstart §0 is fully offline. Live tests stay opt-in. |
| **VI. Performance Budget** | ✅ | ✅ The tool list is static. `pixel-compare` is off by default; if it's ever needed, its size cost is measured before it can be turned on in release builds. |
| **Scope & Technology** | ✅ | ✅ REST only. No bot-logic hosting: raw refuses `applications/…/commands`. No server creation: template import produces a file only. |
| **Workflow gates** | ✅ | ✅ New ADRs: **ADR-004** dispatcher instead of dynamic tool lists (E1); **ADR-005** raw group exception to IV (E2); **ADR-006** writing ids back into the spec file (E9). |

**Result**: PASS, with one justified exception (Complexity Tracking). **Recommended**: run
`/speckit-constitution` to amend principle IV (MINOR, 1.0.0 → 1.1.0) to: "…never raw Discord
JSON, **except in the opt-in `raw` group, whose output is redacted, size-limited and wrapped as
untrusted**." That turns the exception into a rule.

## Project Structure

### Documentation (this feature)

```text
specs/002-extended-toolsets/
├── plan.md              # This file
├── research.md          # E1–E13
├── data-model.md        # tool groups, bulk actions, raw, new spec kinds, ops
├── quickstart.md        # validation scenarios
├── contracts/
│   ├── mcp-tools.md     # dispatcher + 12 groups, inputs, annotations, budgets
│   ├── spec-format.md   # new spec file sections
│   └── cli.md           # env vars, `guildsmith <group> <tool>`, apply flags
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks (not created here)
```

### Source Code (additions to the v1 layout)

```text
crates/
├─ guildsmith-core/src/
│  ├─ spec/{settings,automod,onboarding,welcome,expressions,webhooks,invites,events,messages}.rs
│  ├─ asset.rs              # Asset, type/size rules, hashing of bytes handed in
│  ├─ planner/kinds/        # one diff module per new kind; ordering per data-model §5
│  ├─ raw.rs                # path classifier (RawScope)
│  └─ template.rs           # serialized_source_guild → GuildSpec
├─ guildsmith-app/src/
│  ├─ toolsets.rs           # Toolset registry, ToolsetState, block list, enablement log
│  ├─ bulk.rs               # BulkAction / BulkOutcome, batching, cancellation
│  ├─ assets.rs             # file loading under GUILDSMITH_FILE_ROOTS
│  ├─ writeback.rs          # toml_edit id write-back (E9)
│  └─ services/{moderation,messages,threads,polls,events,invites,voice,automod,
│               webhooks,expressions,settings,raw}.rs
├─ guildsmith-discord/src/
│  ├─ ext/{bans,soundboard,voice,search,events,automod,guild,invites}.rs
│  ├─ raw.rs                # RequestBuilder::raw + audit header + redaction
│  └─ cdn.rs                # asset download for plan comparison
├─ guildsmith-mcp/src/
│  ├─ dispatch.rs           # `toolsets` + `tool_call`
│  └─ tools/{moderation,messages,threads,polls,events,invites,voice,automod,
│            webhooks,expressions,settings,raw}.rs
└─ guildsmith/src/cli/groups.rs   # `guildsmith <group> <tool>` generated from the same registry
plugins/skills/{guild-moderation,guild-community,guild-server-as-code,guild-raw}/SKILL.md
examples/full-server/{guild.toml,assets/}
```

**Structure Decision**: No new crates. Each group is a service module in `app`, a tool module
in `mcp`, and a CLI subcommand generated from the same registry. That keeps the CLI and MCP the
same (FR-008), and the dependency direction stays as in v1.

## Delivery order

Each step ships by itself, following the spec's priorities:

| Step | Contents | Depends on |
|---|---|---|
| 0 Foundation | dispatcher (US8), `BulkAction`, ext request path, extended FakeDiscord, budget gates for each group | v1 M3 |
| 1 | `moderation` (US1) | 0 |
| 2 | `messages`, `threads`, `polls` (US2) | 0 |
| 3 | `events`, `invites`, `voice` (US3) | 0 |
| 4 | `automod` (US4) | 0 |
| 5 | `webhooks` (US5) | 0 |
| 6 | `expressions` (US6), including the E7 CDN check | 0 |
| 7 | `settings` + templates (US7) | 0 |
| 8 | Server as code for every kind (US9), one kind per PR, after that kind's live group | 4–7 and their groups |
| 9 | `raw` (US10) | 0, plus the constitution amendment or the accepted exception |

## Risks

| Risk | Mitigation |
|---|---|
| The CDN re-encodes assets, which would break convergence for expressions | Check early in step 6. If so, use the `pixel-compare` fallback (E7). |
| An "always allow" on `tool_call` in the client covers every tool | The real controls stay inside guildsmith: previews, allowlist, read-only mode. The skills docs explain it. `toolsets(describe)` shows every tool's annotations. |
| twilight releases make ext modules redundant | Delete each ext module once a release covers it, with a test that checks the payload is the same. |
| Unverified slot counts and webhook token behaviour | Show slots as approximate and let Discord's error win. Check `GET webhooks/{id}` token behaviour in step 5; if no token is returned, `webhook_send` only works for webhooks created in this process. |
| `search` needs the MESSAGE_CONTENT privileged intent | `doctor` reports it. The `scan` fallback (E11). |
| Raw access is a broad capability | Its own group, which the operator can block. Writes need confirmation, follow the allowlist and read-only mode, and refuse token routes. |

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| Principle IV, "never raw Discord JSON", broken by `discord_request` (`raw` group) | The user asked for a general raw tool (spec Q2 → C) so guildsmith can do *anything*, including Discord features released after it. Only raw responses can describe endpoints that guildsmith has no typed model for. | **Typed tools only**: anything new would wait for a release. **Read-only raw**: the user asked for writes. The exception is limited: opt-in, operator can block it, tokens removed, 50k-character limit, wrapped as untrusted. Every other tool still follows IV. |
| Default tool set grows from 21 to 23 | The agent needs to enable groups mid-session in every client (spec Q3 → C, FR-003a). That needs always-present dispatcher tools. | **Changing the tool list at runtime**: goes against the MCP 2026-07-28 "side effect" rule, Codex ignores the change by default, and GitHub removed the same feature (E1). Still within the 25-tool and 3k-token budget, so this is noted rather than a violation. |
