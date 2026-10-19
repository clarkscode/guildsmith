# Contract: `guildsmith` command line

One binary serves both the CLI and the MCP server. Every command calls the same
`guildsmith-app` services as the matching MCP tool (FR-032), so policy and output
content are identical; only the presentation differs.

## Global options

| Option | Env var | Meaning |
|---|---|---|
| `--config <path>` | — | Config file. Precedence: env vars > `--config` > `~/.config/guildsmith/config.toml` (FR-034). On Windows the default is `%APPDATA%\guildsmith\config.toml`, on macOS `~/Library/Application Support/guildsmith/config.toml` with `~/.config/guildsmith/config.toml` also accepted. |
| `-g, --guild <id\|name>` | — | Target server. An ID, or an exact server name the bot can see. Ambiguous names fail and list candidates. |
| `--read-only` | `GUILDSMITH_READ_ONLY=1` | Refuse every write (FR-017). |
| `--log <filter>` | `GUILDSMITH_LOG` | `tracing` filter, e.g. `info`, `guildsmith=debug`. Logs always go to **stderr**. |
| `--color <auto\|always\|never>` | `NO_COLOR` honored | Terminal colors for human output. |
| `--json` | — | Machine-readable output on stdout (commands that support it). |

Other settings, from env or the config file only: `DISCORD_TOKEN`,
`GUILDSMITH_GUILDS` (comma-separated allowlist of IDs), `GUILDSMITH_TOOLSETS`.

## Commands

### `guildsmith mcp`
Runs the MCP server on stdio. stdout carries only protocol messages. Accepts
`--toolsets <list>` (overrides `GUILDSMITH_TOOLSETS`). See [mcp-tools.md](mcp-tools.md).

### `guildsmith guilds`
Lists servers the bot can access, filtered by the allowlist. `--json` supported.

### `guildsmith export -g <guild> [-o <path>] [--force]`
Writes the server's structure as a spec file (FR-003/FR-004). Without `-o`, prints to
stdout. Refuses to overwrite an existing file unless `--force`.

### `guildsmith validate <file>`
Checks the file against the schema and its references, offline (FR-006). Prints each
problem with `file:line:col`, a message and a suggestion where available.

### `guildsmith plan -g <guild> <file> [--json] [--detailed-exitcode]`
Prints the plan: ops in order, each with summary and risk, unmanaged items, request
estimate, and the **plan hash** (FR-007, FR-012). Read-only; always allowed in
read-only mode.

### `guildsmith apply -g <guild> <file> [--yes] [--plan-hash <hash>] [--json]`
1. Builds the plan and runs the permission preflight (FR-020). Any preflight problem → exit 3, nothing changed.
2. **Interactive** (no `--yes`): prints the plan and asks for confirmation on a TTY. The hash of the plan that was shown is passed to apply, so if the server changes before the user confirms, apply refuses (exit 3). On a non-TTY without `--yes`, exits 3.
3. **Non-interactive** (`--yes`): requires `--plan-hash` from a plan someone reviewed. `--yes` alone exits 3 with "--yes needs --plan-hash from a reviewed plan" (constitution I). This is how CI pipelines review a plan in one job and apply it in another.
4. Either way, apply takes a fresh snapshot, rebuilds the plan, and refuses (exit 3) unless the hash matches.
5. Applies, printing progress to stderr. Stops at the first error and prints what was applied (FR-013).

### `guildsmith doctor -g <guild>`
Checks the token, allowlist, the bot's permissions per enabled tool group, and role
order (FR-033). Exits 0 if all checks pass, otherwise 3.

### `guildsmith schema [-o <path>]`
Prints the JSON Schema for spec files (FR-005).

### `guildsmith auth set | clear | status`
Stores, removes or reports the token in the OS keyring (FR-035). `set` reads the token
from stdin or a hidden prompt, never from an argument (to keep it out of shell history).
`status` never prints the token.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. For `plan --detailed-exitcode`: no changes. |
| 1 | Error: invalid input, network or Discord error, apply stopped partway. |
| 2 | `plan --detailed-exitcode` only: the plan has changes. |
| 3 | Refused by policy or preflight: allowlist, read-only, missing confirmation, `--yes` without `--plan-hash`, plan-hash mismatch, server changed since plan, missing permissions, not a bot token. |

## Output rules

- The token never appears in any output, including `--log trace` (FR-021).
- Human output is meant to be read by people; `--json` output follows the same
  structures as the MCP tools' `structuredContent` ([mcp-tools.md](mcp-tools.md)).
- Errors print one line of cause plus, where useful, a hint line (`hint: …`).
