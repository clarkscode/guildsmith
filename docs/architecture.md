# Architecture

guildsmith is one Rust binary that is both a CLI and an MCP stdio server. Both are thin
interfaces over the same application services, so a rule enforced for one is enforced for the
other (constitution I and II).

## Layers and dependency direction

```text
guildsmith (binary: clap CLI + `mcp` subcommand)
 ├─▶ guildsmith-mcp      (rmcp tools: translate input/output only)
 ├─▶ guildsmith-discord  (twilight adapter: implements DiscordPort)
 ├─▶ guildsmith-app      (services, policy, preflight, cache, ports)
 └─▶ guildsmith-core     (pure: spec, snapshot, matching, planner, hash, explain)

mcp ─▶ app ─▶ core        discord ─▶ app, core        core ─▶ nothing internal
```

| Crate | Owns | Never does |
|---|---|---|
| `guildsmith-core` | The spec format (`GuildSpec`, parsing with `file:line:col`, the JSON Schema, the deterministic writer), the normalized `Snapshot` and its fingerprint, the permission table (bits 0–52), offline validation, matching (R12, R6 guard), the diff and planner (R7–R10), op ordering (R8), the plan hash (R4), the permission explainer (R13), and a pure `simulate` used by property tests. | I/O of any kind. It depends only on serde, toml, schemars, blake3 and thiserror. |
| `guildsmith-app` | `DiscordPort` and `Clock` (the ports), `Policy` (allowlist, read-only, toolsets), preflight, the confirmation guard, the snapshot cache, name resolution, config and the `Secret` type, and every use case in `services/`. `FakeDiscord` (feature `fake`) is the in-memory port for tests. | Talk to Discord or MCP directly. |
| `guildsmith-discord` | `TwilightDiscord`: maps twilight models to core types, builds snapshots, retries reads only, and sends writes (raw JSON for channels, so fields, parent and the full overwrite list travel in one request). | Let twilight types out of the crate. |
| `guildsmith-mcp` | Tool definitions, toolset registration, output shaping (paging, `fields`, the 50,000-character cap, untrusted wrapping), compact output schemas. | Hold business rules. |
| `guildsmith` | Argument parsing, config loading, logging (stderr only, scrubbed), human rendering, exit codes. | Hold business rules. |

## Ports

`DiscordPort` is the only way guildsmith reaches Discord. Every write method takes the audit-log
reason. Services are generic over `P: DiscordPort`, so tests run the real services against
`FakeDiscord`, which applies the Discord rules guildsmith relies on (role hierarchy, `@everyone`,
managed roles, one `parent_id` change per bulk reorder) and can inject failures.

## Where policy lives

All of it is in `guildsmith-app`; the interfaces only render the result.

| Rule | Where |
|---|---|
| Allowlist (FR-016) | `Policy::ensure_allowed`, called by `services::guilds::resolve` for every server input. `guilds::list` hides servers that aren't allowed. |
| Read-only (FR-017) | `Policy::ensure_writable`, the first thing every writing service does, before any request. |
| Confirmation (FR-018) | `confirm::guarded`: destructive live operations return `{preview, would}` unless confirmed. |
| Plan-hash gate (FR-012, R4) | `services::apply`: fresh snapshot, rebuild the plan, refuse unless the hash matches. |
| Preflight (FR-020) | `preflight::check_plan` / `check_tools` / `check_role_edit`, collecting every problem before refusing. |
| Audit reasons (FR-019) | `policy::audit_reason`, used for every write (`guildsmith: <summary>`, ≤ 512 characters). |
| Not a bot token (FR-022) | The adapter checks `users/@me` once; `ServiceError::from(PortError::NotBotToken)` turns it into a refusal. |
| Token secrecy (FR-021) | `Secret` never prints its value; the log writer scrubs `Authorization` headers and the token. |
| Untrusted content (FR-023) | `shape::wrap_messages` in the MCP layer; services keep user text in its own fields. |

Refusals map to exit code 3 on the CLI and to `isError: true` with a structured `refusal` over MCP.

## The plan lifecycle

```text
spec text ─validate─▶ GuildSpec ─┐
                                  ├─ match (id, then name in scope) ─▶ ops ─▶ order (R8) ─▶ Plan{hash}
live server ─snapshot──────────────┘
apply(hash): fresh snapshot → same plan? → preflight → run ops in order, stop at the first error
```

Reorders carry the desired order rather than positions. Just before sending one, the apply service
takes a fresh snapshot and permutes only the slots the managed items already hold
(`planner::positions`), so unmanaged items keep their places and newly created items land where
the spec puts them.

## Testing without the network

- Core: unit and property tests. `planner_props` applies random plans with `simulate` and checks
  every data-model §3 invariant, including `diff(spec, apply(spec, snapshot))` being empty.
- App: services against `FakeDiscord` (apply, preflight, cache, live tools, doctor, examples).
- Adapter: wiremock recordings for the read path, and request-shape tests for the write path.
- MCP: an in-process rmcp client checks annotations, schemas, refusals, progress and wrapping.
- Binary: CLI tests in-process, stdout purity and the secret-leak harness, which sends a canary
  token through the real adapter to a mock Discord.
- Opt-in live tests (`GUILDSMITH_LIVE_TEST=1`) run the quickstart against a real test server.
