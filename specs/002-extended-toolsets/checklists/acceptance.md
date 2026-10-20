# Acceptance results: extended toolsets (002)

Recorded 2026-10-20 on Linux x86_64 (rustc 1.99.1), from the working tree on branch
`002-extended-toolsets` (uncommitted). Results are tables, not checkboxes: this file records
evidence and is not a gate. Live runs need the two test servers from quickstart.md and are
pending.

## Budgets (T116: SC-002, SC-003, constitution VI)

`cargo build --release -p guildsmith && cargo xtask budgets`:

| Budget | Limit | Measured | Result |
|---|---|---|---|
| Default `tools/list` | exactly 23 tools | 23 | pass |
| Default descriptions | ≤ 3,000 tokens | ≈ 488 | pass |
| Default full `tools/list` | ≤ 9,000 tokens | ≈ 8,493 | pass (tight: 94%) |
| Largest group (`moderation`) | ≤ 15 tools, ≤ 1,500 description tokens | 10 tools, ≈ 210 | pass |
| `all` | exactly 84 tools, ≤ 15,000 description tokens | 84, ≈ 1,844 | pass |
| Cold start to `tools/list`, default / `all` (median of 20) | < 50 ms | 17.5 ms / 28.8 ms | pass |
| Stripped release binary | < 10 MiB | 9.92 MiB (`opt-level = "z"`) | pass (tight: 99%) |

The binary was 10.95 MiB with v1's `opt-level = "s"`, over budget; `"z"` was adopted, with
the same cold start. Headroom is now under 0.1 MiB, so the next feature needs a size plan (see
the completion report). `xtask budgets` now fails when `all` isn't 84 tools or a group adds
none, which catches a stale binary (an earlier run measured one and passed).

## Quickstart scenarios (T118)

| § | Scenario | Status | Evidence |
|---|---|---|---|
| 0 | Offline checks | pass | `cargo test --workspace --all-features`: 89 result groups, 385 tests, all pass. `cargo clippy … -D warnings` clean. `cargo fmt --check`, `xtask schema --check`, `xtask check-plugins` pass. Budgets above. |
| 1 | Groups and dispatch (SC-002/003/013) | pass offline; live pending | `mcp/tests/dispatch.rs`, `app/tests/toolsets.rs`; budgets above. Live: `tests/live/community.rs`. |
| 2 | Moderation (SC-004/005/008) | pass offline; live pending | `app/tests/moderation.rs` (previews, 450 bans in 3 batches, outcome sums, `sc008_fifty_bans_are_one_request_and_fast`: 50 bans in 1 request), `bulk_props`. Live: `tests/live/moderation.rs` (needs `GUILDSMITH_LIVE_ALTS`). |
| 3 | Content, community, server areas (FR-008) | pass offline; live pending | Service and protocol tests per group, CLI group tests; `secret_leak::every_group_keeps_bot_and_webhook_tokens_out` (SC-006). Live: `tests/live/{community,events,automod,webhooks,expressions,settings}.rs`. |
| 4 | Whole server as code (SC-010/011) | pass offline; live pending | `core/tests/converge_kinds.rs` (every kind, 64 property cases), `app/tests/examples.rs` (full-server example), `app/tests/server_as_code.rs` (export with files → apply elsewhere → both plans empty; no webhook URL), `app/tests/writeback.rs`. Live: `tests/live/server_as_code.rs` (needs `GUILDSMITH_LIVE_EMPTY`). |
| 5 | Raw requests (SC-012) | pass offline; live pending | `core/tests/raw_classifier.rs`, `app/tests/raw.rs`, `mcp/tests/raw.rs`, `guildsmith/tests/cli_raw.rs`. Live: `tests/live/raw.rs`. |

## Agent run and timing (T117: SC-007, SC-008)

| Check | Status | Evidence |
|---|---|---|
| SC-008: 50-account ban under 2 minutes | pass offline; live pending | One bulk-ban request for 50 ids (`sc008_fifty_bans_are_one_request_and_fast`); the live run times it (`tests/live/moderation.rs`). |
| SC-007: scripted 20-task agent run, ≥ 2 tasks per story, no failed tool choice | pending | Needs an agent session against the live test servers. |

## Unverified research items

| Item | Status |
|---|---|
| T063: does `GET webhooks/{id}` return tokens for webhooks the bot didn't make? | pending live check (research §E-verification) |
| T074: does the CDN return uploaded emoji/sticker/sound bytes unchanged? | pending live check; `tests/live/expressions.rs` asserts it, and plan comparison is by bytes until then |
