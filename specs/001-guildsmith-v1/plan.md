# Implementation Plan: guildsmith v1

**Branch**: `001-guildsmith-v1` | **Date**: 2026-10-19 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/001-guildsmith-v1/spec.md`, plus the project plan in
`PLAN.md`

## Summary

**Terminology**: the spec's "tool group" is called a **toolset** in this plan, the contracts, the
tasks, the code and the configuration (`GUILDSMITH_TOOLSETS`). They are the same thing.

guildsmith is a single Rust binary. It is both a CLI and an MCP stdio server for managing the
structure of Discord servers: roles, categories, channels and permission overwrites.

It offers two workflows:
1. **Server as code**: export → validate → plan → apply. The plan is a reviewable diff, and the
   result converges: running plan after apply shows no changes.
2. **Day-to-day changes**: a small set of typed live tools for agents (21 by default).

Both interfaces call the same application services. Those services enforce the safety policy:
server allowlist, read-only mode, previews before destructive actions, a stateless plan-hash
gate, permission preflight, and an audit-log reason on every write.

The technical approach follows PLAN.md §3–4: a hexagonal workspace, with twilight for Discord
REST and rmcp for MCP. Research updated it for:
- the current MCP revision, 2026-07-28 (R3);
- Discord channel obfuscation from 2026-11-16, which makes the bot need Administrator for spec
  apply (R6);
- the bot not being able to grant permissions it doesn't have (R7);
- one category move per bulk reorder request (R8);
- shipping npm per-platform packages ourselves instead of using dist's postinstall installer
  (R15).

## Technical Context

**Language/Version**: Rust, edition 2024. Built with stable 1.99. MSRV 1.97 (N-2, R1).

**Primary Dependencies**:
- Discord: twilight-http / twilight-model 0.17.1, twilight-validate 0.17.0, with rustls (R2).
- MCP: rmcp 3.5.x (`server`, `macros`, `transport-io`).
- Async runtime: tokio 1.x.
- CLI: clap 4.6.
- Serialization and spec files: serde, toml 1.x, toml_edit 0.25, schemars 1.x.
- Errors: thiserror 2 in libraries, anyhow in the binary.
- Logging: tracing and tracing-subscriber, to stderr.
- Hashing: blake3.
- Secrets: keyring 4.x (optional feature).

**Storage**: None on the server side. Spec files are on the user's disk; config is in
`~/.config/guildsmith/config.toml`; the token is in an env var, the OS keyring or the config
file. Snapshots are cached in memory only (R18).

**Testing**:
- cargo test, with proptest (core invariants) and insta (snapshot tests for exports, plans and
  tool output).
- wiremock for the Discord adapter.
- An in-memory `FakeDiscord` for services.
- rmcp in-process client for MCP protocol tests.
- Opt-in live tests (`GUILDSMITH_LIVE_TEST=1`).

**Target Platform**: Linux x64/arm64 (static musl), macOS x64/arm64, Windows x64. Clients:
Claude Code, Codex, and any MCP stdio client.

**Project Type**: CLI + MCP server (one binary) on top of library crates.

**Performance Goals**:
- Cold start to the first `tools/list` response in under 50 ms.
- No network I/O at startup.
- Apply is limited by Discord's rate limits, not by CPU.

**Constraints**:
- Stripped binary under 10 MB.
- Tool descriptions within 3k tokens, and the whole `tools/list` within 9k (R16).
- stdout carries only the MCP protocol.
- The token never appears in any output.
- REST only; bot tokens only.

**Scale/Scope**: Discord's limits: up to 250 roles, 500 channels, and 50 channels per category in
a server. v1 covers milestones M1–M4: the `core` toolset (21 tools), the `moderation` toolset
(4 tools), 6 CLI commands plus `guilds`, `validate` and `auth`, and 2 agent plugins.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Pre-research | Post-design |
|---|---|---|---|
| I. Safe by Default | Allowlist, read-only mode, confirm previews, plan-hash gate with server-change detection, audit reasons, preflight that reports everything, token never output, user content marked untrusted. All in `guildsmith-app`. | ✅ Planned | ✅ R4 (stateless hash that includes the snapshot fingerprint), R5, R6/R7 (Administrator profile, visibility guard), R14 (`Secret` newtype, scrubbing), R19. Policy lives in `guildsmith-app::policy`, so the CLI and MCP can't bypass it. |
| II. Pure Core, Thin Adapters | Core has no I/O and doesn't depend on Discord, MCP or CLI crates; twilight stays in `guildsmith-discord`. | ✅ Planned | ✅ Core depends only on serde, schemars, blake3 and thiserror. The permission table lives in core (R2), so twilight's missing flag doesn't leak out. A git pin of twilight, if needed, is limited to the adapter crate. |
| III. Plans Converge | `diff ∘ apply = ∅`, deterministic plans, matching by id then name, no state file, prune off by default, apply stops at the first error and can resume. | ✅ Planned | ✅ Data-model §3 invariants are property-tested against `FakeDiscord`. The visibility guard (R6) stops hidden channels being recreated as duplicates. Replace ops (R10) are destructive and need explicit opt-in. |
| IV. Lean Agent Interface | At most 25 default tools, a token budget, compact paged results, correct annotations, logs to stderr. | ✅ Planned | ✅ 21 `core` tools plus 4 opt-in `moderation` tools. Budgets are checked in CI (R16). `anthropic/requiresUserInteraction` is set on hard deletes (R5). |
| V. Tested Without the Network | Property tests, FakeDiscord, recorded fixtures, protocol tests, opt-in live tests, a regression test for every bug fix. | ✅ Planned | ✅ R16. Quickstart §0 runs fully offline. |
| VI. Performance Budget | Under 50 ms start, under 10 MB binary, checked in CI. | ✅ Planned | ✅ R17: lazy HTTP client, no startup network calls, release profile settings, `cargo xtask budgets` gate. |
| Scope & Technology | No server creation, no user tokens, REST only, stack per PLAN.md, `unsafe` forbidden, cargo deny/audit, MIT OR Apache-2.0. | ✅ | ✅ Every dependency is MIT/Apache-compatible; checked by `cargo deny`. |
| Workflow gates | CI runs fmt, clippy `-D warnings`, tests, deny, minimal-versions, budgets and a dist dry-run. ADRs record changes to PLAN.md §3. | ✅ | ✅ ADRs needed: **ADR-001** (Administrator required for spec apply, R6/R7), **ADR-002** (our own npm platform packages, not dist's installer, R15), **ADR-003** (MCP 2026-07-28 target, R3). |

**Result**: PASS, with no violations. Complexity Tracking is not needed.

## Project Structure

### Documentation (this feature)

```text
specs/001-guildsmith-v1/
├── plan.md              # This file
├── research.md          # Phase 0: decisions R1–R21
├── data-model.md        # Phase 1: spec, snapshot, plan, op, policy types
├── quickstart.md        # Phase 1: validation scenarios
├── contracts/
│   ├── cli.md           # commands, flags, exit codes
│   ├── mcp-tools.md     # toolsets, inputs, outputs, annotations, budgets
│   ├── spec-format.md   # guild.toml syntax and compatibility
│   └── plugins.md       # Claude Code, Codex, npm packaging
├── checklists/requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks; not created here)
```

### Source Code (repository root)

```text
Cargo.toml                         # workspace: shared deps, [workspace.lints], release profile
rust-toolchain.toml                # stable; MSRV in [workspace.package] rust-version = "1.97"
deny.toml
dist-workspace.toml                # cargo-dist 0.33 config (5 targets)
crates/
├─ guildsmith-core/                # pure: no I/O
│  └─ src/{spec/, permissions.rs, snapshot.rs, matching.rs, diff.rs,
│          planner/{mod,order,ops}.rs, explain.rs, validate.rs, hash.rs}
├─ guildsmith-app/
│  └─ src/{ports.rs (DiscordPort, Clock), policy.rs, preflight.rs, resolve.rs,
│          config.rs, secret.rs, services/{export,plan,apply,live}.rs}
│     tests/ + src/fake.rs         # FakeDiscord (behind a `fake` feature, used by other crates' tests)
├─ guildsmith-discord/
│  └─ src/{client.rs, map/{role,channel,overwrite,guild}.rs, snapshot.rs, retry.rs}
│     tests/fixtures/              # wiremock recordings
├─ guildsmith-mcp/
│  └─ src/{server.rs, toolsets.rs, tools/{guild,spec,roles,channels,perms,members,
│          messages,moderation}.rs, shape.rs (paging, fields, untrusted wrapping)}
│     tests/protocol.rs            # list/call each tool, check schemas and token budget
└─ guildsmith/                     # binary: clap CLI + `mcp` subcommand
   └─ src/{main.rs, cli/{export,plan,apply,doctor,validate,schema,guilds,auth}.rs, render.rs}
xtask/                             # cargo xtask: schema, budgets, sync-plugins, npm-pack
schema/guild.schema.json           # generated; CI checks for drift
plugins/
├─ skills/                         # the single source for skills (R15)
├─ claude-code/{.claude-plugin/plugin.json, .mcp.json, skills/}
└─ codex/{.codex-plugin/plugin.json, .mcp.json, skills/}
.claude-plugin/marketplace.json
.agents/plugins/marketplace.json
npm/{guildsmith/, platforms/<5 dirs>/}
examples/{community,dev-team,gaming}.toml
tests/live/                        # opt-in e2e (GUILDSMITH_LIVE_TEST=1)
docs/{architecture.md, tools.md, spec-reference.md, codex.md, adr/}
.github/workflows/{ci.yml, release.yml, nightly-live.yml}
```

**Structure Decision**: This is the workspace from PLAN.md §4.1, with three additions:
- an `xtask/` crate for code generation and the CI budget gates;
- the marketplace manifests at the repo root, which keeps PLAN.md Q1's one-repo approach;
- `plugins/skills/` as the single source for skills.

The `.agents/` directory at the repo root already holds Spec Kit's Codex integration files. The
Codex marketplace file sits next to them under `.agents/plugins/`.

**Dependency direction**:
- `guildsmith` (binary) → `mcp`, `app`, `discord`, `core`
- `mcp` → `app`, `core`
- `discord` → `app` (it implements the port), `core`
- `app` → `core`
- `core` → nothing internal

## Milestone mapping

| Milestone | Spec stories | Key research items |
|---|---|---|
| M0 Scaffold | — | R1, R15 (crate and npm names, dist config), R16/R17 CI gates (initially with an empty tool list) |
| M1 Read path | US1, US5 (doctor) | R2, R6.3, R11, R12, R13, R14 |
| M2 Plan/apply | US2, US3 (preflight, plan-hash gate) | R4, R6.2, R7, R8, R9, R10 |
| M3 Live tools + safety | US3, US4 | R5, R18, R19, R20 |
| M4 Ship | US6 | R15 |

## Risks

| Risk | Mitigation |
|---|---|
| twilight release lag (no crates.io release for about 10 months) | Use 0.17.1. Pin a git revision only inside the adapter if needed (with an ADR). `DiscordPort` limits how much a library switch costs. |
| rmcp 3.5.1 may not include protocol 2026-07-28 | Nothing depends on the new revision. Fall back to 2025-11-25 (R3). |
| Channel obfuscation from 2026-11-16 | Require Administrator for apply, plus the visibility guard (R6). The fixtures include the hidden-channel case. |
| Unofficial rate limits (name/topic edits, role creation) | One PATCH per channel, warnings on volume, no blind retries, progress reporting (R7/R9). |
| keyring on musl | Check in M0. Fall back to the `linux-keyutils` store (R14). |
| Requiring Administrator may put off cautious users | `doctor` explains why. Live tools work with narrower permissions per toolset. ADR-001 records the trade-off. |

## Complexity Tracking

No constitution violations, so nothing to justify.
