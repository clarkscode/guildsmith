# Prerequisites: v1 baseline (T001)

Checked 2026-10-20 on Linux x86_64 (rustc 1.99.1) at commit `693793e`, on a clean tree,
before any 002 change. Results are recorded as evidence, not as a gate.

| Check | Result | Evidence |
|---|---|---|
| `cargo test --workspace --all-features` | pass | 45 test result groups (including doc-tests), 188 tests, 0 failed. |
| `cargo xtask budgets` (release build) | pass | Cold start 11.51 ms (< 50 ms), binary 7.20 MiB (< 10 MiB), default 21 tools ≈ 437 description tokens, `core,moderation` 25 tools ≈ 499 tokens. |
| `FakeDiscord` exists | yes | `crates/guildsmith-app/src/fake.rs` (876 lines, feature `fake`). |
| `GUILDSMITH_TOOLSETS` parsing exists | yes | `policy::parse_toolsets` in `crates/guildsmith-app/src/policy.rs`, read by `config::load` in `crates/guildsmith-app/src/config.rs` (env, then the config file's `toolsets`), and `guildsmith mcp --toolsets`. |
| Toolset mechanism | yes | `Toolset { Core, Moderation }`; `crates/guildsmith-mcp/src/toolsets.rs` registers enabled groups in a fixed order; `tools/list` is fixed for the process with a 1-hour `ttlMs`. |
| Budget gate | yes | `xtask/src/budgets.rs` (default exactly 21 tools; `core,moderation` ≤ 25). |

v1 is implemented through milestone M3 (and M4 packaging), so 002 can build on it.
