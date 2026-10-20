# Acceptance results: guildsmith v1

Recorded 2026-10-19 on Linux x86_64 (rustc 1.99.1), from the working tree after tasks T050–T101.
Offline results re-verified 2026-10-20 at commit 693793e (same toolchain); figures below are from that run.
Results are tables, not checkboxes: this file records evidence and is not a gate.

## Budgets (T102: SC-003, SC-004, SC-005)

`cargo build --release -p guildsmith && cargo xtask budgets`:

| Budget | Limit | Measured | Result |
|---|---|---|---|
| Cold start to the first `tools/list` (median of 20) | < 50 ms | 11.51 ms | pass |
| Stripped release binary | < 10 MiB | 7.20 MiB (7,549,864 bytes) | pass |
| `core` toolset size | exactly 21 | 21 | pass |
| `core,moderation` toolset size | ≤ 25 | 25 | pass |
| Tool descriptions, default | ≤ 3,000 tokens | ≈ 437 | pass |
| Tool descriptions, `core,moderation` | ≤ 3,000 tokens | ≈ 499 | pass |
| Full `tools/list`, default | ≤ 9,000 tokens | ≈ 7,535 | pass |
| Full `tools/list`, `core,moderation` | ≤ 9,000 tokens | ≈ 8,827 | pass |

Tokens are estimated as UTF-8 bytes ÷ 3.5 (research R16). Output schemas are sent without
descriptions or numeric formats to stay within the budget; input descriptions are kept.

## Quickstart scenarios (T103)

| § | Scenario | Status | Evidence |
|---|---|---|---|
| 0 | Build and offline checks | pass | `cargo test --workspace --all-features`: 45 test result groups (including doc-tests), 188 tests, all pass. `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean. MSRV: `cargo +1.97 check --workspace --all-targets --all-features` passes. Budgets above. |
| 1 | Export round trip (SC-001) | pass offline; live run pending | `export_roundtrip` (recorded fixtures + 128 generated snapshots through the real writer and parser) and `planner_props::exports_plan_to_nothing`. Live: `tests/live/export.rs`. |
| 2 | Apply converges (SC-002) | pass offline; live run pending | `examples` (all three examples apply to an empty `FakeDiscord` and re-plan to nothing); `planner_props` convergence properties (20,000 cases each checked during development). Live: `tests/live/apply.rs`. |
| 3 | Server changed between plan and apply | pass offline; live run pending | `apply::a_server_edited_between_plan_and_apply_is_refused`, `cli_plan::interactive_apply_uses_the_shown_plans_hash`. Live: `tests/live/apply.rs`. |
| 4 | Policy (SC-007, SC-008) | pass offline; live run pending | `policy.rs` (every write tool and CLI command, allowlist and read-only, zero requests before refusal), `core_tools::destructive_tools_preview_without_confirm`, `spec_tools::spec_apply_refuses_a_made_up_hash`. Live: `tests/live/policy.rs`. |
| 5 | Preflight reports every problem (SC-009) | pass offline; live run pending | `preflight::every_problem_is_reported_together_and_nothing_changes`, `doctor.rs`. Live: `tests/live/preflight.rs` (needs the manual bot setup in its docs). |
| 6 | Hidden channel guard (R6) | pass offline; live run pending | `matching::hidden_id_is_a_hard_error_without_administrator`. Live: `tests/live/hidden_channel.rs`. |
| 7 | Agent day-to-day (SC-004, SC-012) | tools pass offline; agent transcript pending | `core_tools.rs`, `live.rs`; 21 / 25 tools confirmed by `budgets`. SC-012 needs an agent session against a live server, judged from its transcript. Live: `tests/live/agent.rs`. |
| 8 | Install channels (SC-010, SC-011) | pending | Needs the crates.io and npm names reserved and a first release (docs/release.md, owner-only). `.github/workflows/install-smoke.yml` runs after the Release workflow. The install-to-first-plan time (SC-010, < 10 minutes) can only be measured on that release. |
| 9 | Secret hygiene (SC-006) | pass | `secret_leak.rs`: the canary token travels through the real twilight adapter to a mock Discord in every request, across every CLI command and all 25 tools at trace level, including failures, and appears in no output, error or log. `cli_doctor::doctor_with_the_real_binary_never_prints_the_token`. |

### To finish §1–§8

1. Create a dedicated test server, invite the bot with Administrator, and set
   `DISCORD_TOKEN`, `GUILDSMITH_GUILDS`.
2. `GUILDSMITH_LIVE_TEST=1 GUILDSMITH_LIVE_DESTRUCTIVE=1 cargo test -p guildsmith --test live -- --test-threads=1`
   (or add the `LIVE_DISCORD_TOKEN` / `LIVE_GUILD_ID` secrets and the `live-test-server`
   environment for `.github/workflows/nightly-live.yml`).
3. For §5 and §6, follow the setup notes in `tests/live/preflight.rs` and
   `tests/live/hidden_channel.rs`.
4. For §8, follow `docs/release.md`, then time a fresh install to the first plan per channel.
