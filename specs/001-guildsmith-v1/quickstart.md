# Quickstart: validating guildsmith v1

These scenarios show, end to end, that the spec's user stories and success criteria hold. Each
scenario lists what it proves. Contracts:
[cli.md](contracts/cli.md) · [mcp-tools.md](contracts/mcp-tools.md) ·
[spec-format.md](contracts/spec-format.md) · [plugins.md](contracts/plugins.md).

## Prerequisites

- Rust 1.99+ (stable) for building. MSRV is 1.97.
- A Discord application with a bot token and a **dedicated test server**, created by you. The
  bot must be invited with `Administrator` for spec apply (R6/R7).
- `export DISCORD_TOKEN=…` and `export GUILDSMITH_GUILDS=<test server id>`.
- For live tests: `export GUILDSMITH_LIVE_TEST=1`. Without it, live tests are skipped.

## 0. Build and offline checks

```bash
cargo build --release
cargo test --workspace                    # core property tests, FakeDiscord, wiremock, MCP protocol
cargo xtask budgets                       # SC-003/004/005: startup, tools/list tokens, binary size
```
**Expect**: all tests pass. `budgets` reports a cold start under 50 ms to `tools/list`,
descriptions within 3k tokens, and a binary under 10 MB.

## 1. Export round trip (US1, SC-001)

```bash
guildsmith doctor  -g "$GUILDSMITH_GUILDS"                  # all checks pass
guildsmith export  -g "$GUILDSMITH_GUILDS" -o lab.toml
guildsmith validate lab.toml                                # no problems
guildsmith plan    -g "$GUILDSMITH_GUILDS" lab.toml --detailed-exitcode; echo $?
```
**Expect**: exit code `0` and "No changes." Running export a second time produces a
byte-identical file.

## 2. Apply converges (US2, SC-002)

On an **empty** test server:
```bash
guildsmith plan  -g "$GUILDSMITH_GUILDS" examples/community.toml      # note the plan hash
guildsmith apply -g "$GUILDSMITH_GUILDS" examples/community.toml --plan-hash <hash> --yes
guildsmith plan  -g "$GUILDSMITH_GUILDS" examples/community.toml --detailed-exitcode; echo $?
```
**Expect**:
- In the first plan, roles come before channels and the plan is marked safe.
- Apply shows progress, and every change appears in the server's audit log with a
  `guildsmith:` reason.
- The final plan exits `0`.

Repeat with `examples/dev-team.toml` and `examples/gaming.toml`.

## 3. Server changed between plan and apply (US2 scenario 3)

1. Run `plan` and note the hash.
2. Rename any channel by hand in Discord.
3. Run `apply --plan-hash <old hash> --yes`.

**Expect**: exit `3` with "server changed since plan; run plan again". Nothing changes.

## 4. Policy (US3, SC-007/008)

```bash
GUILDSMITH_READ_ONLY=1 guildsmith apply -g "$GUILDSMITH_GUILDS" lab.toml --plan-hash <hash> --yes; echo $?   # 3 (read-only)
guildsmith apply -g "$GUILDSMITH_GUILDS" lab.toml --yes; echo $?   # 3: --yes needs --plan-hash
guildsmith export -g <a server not in the allowlist>; echo $?                           # 3
```
Over MCP (any client, or `cargo test -p guildsmith-mcp policy`):
- Call `channel_delete` without `confirm`. **Expect** a preview, and the channel still exists.
- Call `spec_apply` with a made-up `plan_hash`. **Expect** a refusal.

## 5. Preflight reports every problem (US3 scenario 5, US5, SC-009)

1. Remove `Administrator` and `Manage Roles` from the bot's role.
2. Move a spec role above the bot's role.
3. Run `doctor` and `plan`/`apply`.

**Expect**: one report listing *both* problems, with fixes, before any change. Exit `3`.

## 6. Hidden channel guard (R6)

Use a bot **without** Administrator, and a spec that lists a private channel by `id` that the bot
can't view. Run `plan`.

**Expect**: the error "missing or hidden from the bot". It is never planned as a create.

## 7. Agent day-to-day (US4, SC-004/012)

Start the MCP server: `guildsmith mcp`, or install the plugin (§8). Ask the agent:
1. "List the roles and show the channel tree."
2. "Create #announcements under Community."
3. "Give <member> the Moderator role."
4. "Why can't @everyone see #mod-log?"
5. "Post 'hello' in #general, then read the last 5 messages."

**Expect**:
- Every task is done with `core` tools only.
- Step 4 names the `@everyone` deny overwrite.
- Step 5's messages come back wrapped as untrusted data.
- `tools/list` shows 21 tools, or 25 with `GUILDSMITH_TOOLSETS=core,moderation`.

## 8. Install channels (US6, SC-010/011)

On clean Linux, macOS and Windows machines (the CI matrix plus one manual pass per OS before a
release):

| Channel | Command | Expect |
|---|---|---|
| Prebuilt | dist shell/ps1 installer | `guildsmith --version` works |
| npm | `npx -y guildsmith mcp` (also with `--ignore-scripts` and pnpm) | the server starts, with no postinstall |
| crates.io | `cargo install guildsmith` | builds on MSRV 1.97 |
| Claude Code | `/plugin marketplace add clarkscode/guildsmith` → install → enter token | tools and `/guild-plan` are available |
| Codex | `codex plugin marketplace add clarkscode/guildsmith`, with `DISCORD_TOKEN` set | Codex lists the tools |

Time from install to the first plan of your own server: **under 10 minutes** (SC-010).

## 9. Secret hygiene (SC-006)

```bash
cargo test --workspace secret_leak        # seeds a fake token; scans all logs and outputs
DISCORD_TOKEN=gs-canary-7f3a guildsmith doctor -g "$GUILDSMITH_GUILDS" --log trace 2>&1 | grep -c gs-canary-7f3a  # 0
```
