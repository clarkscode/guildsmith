# guildsmith — project plan

> Fast Rust MCP server and CLI for building and managing Discord servers, with
> plugins for Claude Code and Codex.

Status: planning (2026-10-19). Nothing is implemented yet.

---

## 1. Why this exists

Every Discord MCP server available today is a community project, mostly
written in TypeScript and run through `npx`. Section 12 lists the research
this is based on.

- **Too many tools.** The biggest one exposes about 139 tools. Every tool's
  description takes up space in the agent's context on every turn, which costs
  tokens and makes the agent worse at choosing the right tool.
- **No plan step.** The agent has to build a server one call at a time
  ("create role", "create channel", ...). Nothing shows what will change
  before it happens, and nothing can repeat the result.
- **Weak safety.** Only one of them, PaSympa, has a guild allowlist and
  previews for destructive actions.
- **Node runtime.** They start slowly, use more memory, and each install pulls
  in a large dependency tree.

guildsmith fixes this with two parts that share one engine:

1. **Live tools.** A small set of well-chosen, typed MCP tools for day-to-day
   operations.
2. **Server as code.** A `guild.toml` file describes the server's structure
   (roles, categories, channels, permissions). guildsmith can **export** an
   existing server to this file, **plan** the changes (a diff), and **apply**
   them, like Terraform does for infrastructure. Agents and people can review
   the plan before anything changes.

## 2. Goals and non-goals

### Goals

- One static Rust binary that starts in under 50 ms and uses little memory.
- Expose fewer, better tools by default (about 25). More tool groups can be
  turned on when needed.
- `export → plan → apply` is reliable: running `apply` and then `plan` again
  shows no changes.
- Safe by default: a guild allowlist, previews for destructive actions,
  a read-only mode, and a reason recorded in Discord's audit log for every
  change.
- A clean, layered architecture where the domain logic and planner are
  testable with no network access.
- Easy installs: Claude Code plugin, Codex config, `npx`, `cargo install`,
  and prebuilt binaries.

### Non-goals

- **Creating servers.** Discord removed bot server creation on 2025-07-28. A
  person creates the server, then guildsmith takes over.
- **User tokens or self-bots.** Only bot tokens are supported. Automating a
  personal account breaks Discord's terms of service.
- **Gateway / real-time events in v1.** v1 uses only the REST API. See
  Milestone 5.
- **Being a chat bot framework.** For slash-command bots, use poise or
  serenity. guildsmith manages servers; it doesn't host bot logic.

## 3. Key decisions

| # | Decision | Choice | Why |
|---|---|---|---|
| D1 | Language | Rust (edition 2024) | Speed, a single binary, and strong typing for the domain model. |
| D2 | Discord library | **twilight** (`twilight-http`, `twilight-model`, `twilight-validate`) | Modular: use only the REST client and models. Built-in rate limiter. See §3.1. |
| D3 | MCP SDK | **`rmcp`**, the official Rust SDK from `modelcontextprotocol/rust-sdk` | Maintained by the protocol authors. Supports stdio and HTTP. Tools are declared with macros. |
| D4 | Transport | stdio first; streamable HTTP later behind a feature flag | Claude Code and Codex both start stdio servers as child processes. |
| D5 | Connection model | REST only in v1 | Stateless, starts instantly, needs no privileged intents, and covers all admin work. |
| D6 | Spec format | TOML (`guild.toml`), with a published JSON Schema | Readable and easy to diff. The schema gives editor autocompletion and lets agents validate files. |
| D7 | Async runtime | tokio | Required by twilight and rmcp. |
| D8 | CLI | clap (derive) | Standard choice. One binary handles both the CLI and the MCP server. |
| D9 | Errors | `thiserror` in library crates, `anyhow` only in the binary | Library errors are typed. The binary can add context freely. |
| D10 | Logging | `tracing`, writing to **stderr** only | stdout carries the MCP protocol, so writing anything else there corrupts it. |
| D11 | Distribution | cargo-dist (GitHub Releases), crates.io, an npm launcher, and a plugin marketplace | See §9. |
| D12 | License | MIT OR Apache-2.0 | Standard dual license in the Rust ecosystem. |

### 3.1 Discord library options

| Option | What it is | Pros | Cons | Fit |
|---|---|---|---|---|
| **twilight** ✅ | A set of separate crates: `http`, `model`, `gateway`, `cache-inmemory`, `validate`, … | Pick only the parts needed. Built-in REST rate limiter. Low-level and predictable. Adding the gateway later means adding one crate. | More code to write by hand. No high-level helpers. Smaller community than serenity. | Best fit for a lean REST tool with clean layers. |
| serenity | All-in-one library (HTTP, gateway, cache, models) | Largest community, many examples, quick start. | Heavier. Ties HTTP, gateway and cache together. Its design favors long-running bots. | Better for bots than for an admin tool that runs and exits. |
| poise | A command framework built on serenity | Very good for slash-command bots. | Built for writing bots, not managing servers. Pulls in all of serenity. | Not relevant (see non-goals). |
| Own client (`reqwest` + hand-written models) | Our own client from scratch | Full control. Smallest possible dependency tree. | We'd have to write and maintain models, the rate limiter, and validation, and keep up with Discord API changes. | Too much maintenance. Only worth it if twilight gets in the way. |

**How we protect the choice:** all Discord calls go through a `DiscordPort`
trait, and twilight is used only in `guildsmith-discord`. Moving to another
library means rewriting that one crate.

## 4. Architecture

Hexagonal ("ports and adapters"). Dependencies point inward: the core knows
nothing about Discord, MCP or the CLI.

```
             ┌───────────────┐   ┌───────────────┐
             │ guildsmith-mcp│   │ guildsmith-cli│     ← interface layer (adapters in)
             │ (rmcp tools)  │   │   (clap)      │
             └──────┬────────┘   └──────┬────────┘
                    └────────┬──────────┘
                     ┌───────▼────────┐
                     │ guildsmith-app │                ← use cases: export, plan, apply,
                     │  (services)    │                  live ops, policy/safety checks
                     └───────┬────────┘
                ┌────────────┼──────────────┐
        ┌───────▼───────┐            ┌──────▼─────────────┐
        │guildsmith-core│            │ guildsmith-discord │ ← adapter out (twilight-http)
        │ domain, spec, │◄───────────│ implements          │
        │ diff/planner  │  port trait│ DiscordPort        │
        └───────────────┘            └────────────────────┘
```

### 4.1 Workspace layout

```
guildsmith/
├─ Cargo.toml                  # workspace; shared deps, lints, profile
├─ crates/
│  ├─ guildsmith-core/         # pure: domain model, spec (serde), validation,
│  │                           #   snapshot, diff, planner, op ordering. No IO.
│  ├─ guildsmith-app/          # use cases + ports (DiscordPort, Clock), policy
│  │                           #   (allowlist, read-only, dry-run), name resolution
│  ├─ guildsmith-discord/      # twilight-http adapter: DiscordPort impl, model
│  │                           #   mapping, retry, permission preflight
│  ├─ guildsmith-mcp/          # rmcp server: tool groups, schemas, output shaping
│  └─ guildsmith/              # binary: clap CLI + `guildsmith mcp` subcommand
├─ schema/guild.schema.json    # generated from core via schemars
├─ plugins/
│  ├─ claude-code/             # .claude-plugin/plugin.json, .mcp.json, skills/, commands/
│  └─ codex/                   # config snippet, skills, (plugin manifest if supported)
├─ npm/                        # npx launcher + per-platform binary packages
├─ examples/                   # sample guild.toml files (community, dev team, game)
├─ tests/                      # e2e: recorded HTTP + optional live test guild
└─ docs/                       # architecture, tool reference, spec reference, ADRs
```

### 4.2 Core domain (`guildsmith-core`)

- **Types:** `GuildSpec`, `RoleSpec`, `CategorySpec`, `ChannelSpec` (text,
  voice, forum, announcement, stage), `PermissionOverwriteSpec`, and a
  `Permissions` set with names that can be read in both directions (from
  Discord's numeric flags and from names).
- **Snapshot:** a normalized view of a server's live state, built by the
  adapter.
- **Matching (no state file):** spec items are matched to live items by an
  optional `id` (written in by `export`), then by a unique name within their
  scope. If a match is ambiguous, plan fails with a clear error. This avoids
  Terraform's separate state file.
- **Ownership:** only items listed in the spec are managed. Items on the
  server but not in the spec are reported as *unmanaged* and are only deleted
  when `prune = true` (per kind). The default is to never delete.
- **Planner:** `diff(spec, snapshot) -> Plan`, where a `Plan` is an ordered
  list of `Op`s (`CreateRole`, `UpdateChannel`, `SetOverwrites`,
  `ReorderChannels`, `Delete*`, …). Each `Op` has a short summary that can be
  shown to people, and a risk level (`safe`, `destructive`).
- **Ordering rules:**
  1. Roles before channels, because permission overwrites refer to roles.
  2. Categories before their channels.
  3. Positions are set in one bulk update at the end.
  4. Deletes come last.
- **Invariant (tested):** `diff(spec, apply(spec, s)) == ∅`.

### 4.3 Application layer (`guildsmith-app`)

- **Ports:** `DiscordPort` (async trait: fetch snapshot, CRUD per kind,
  bulk-reorder, current member/permissions) and `Clock`.
- **Services:**
  - `ExportService` turns a snapshot into a spec.
  - `PlanService` combines spec and snapshot into a plan.
  - `ApplyService` runs a plan, reports progress, and stops on the first
    error. It reports exactly what was applied, so running it again continues
    from where it stopped (the next plan picks up the remainder).
  - Live ops services wrap the individual tools.
- **Policy, checked once here for both MCP and CLI:**
  - The guild allowlist.
  - Read-only mode.
  - Destructive ops require `confirm: true`, otherwise they return a preview.
  - Every write records a reason in the audit log
    (`guildsmith: <summary>`).
  - **Permission preflight:** before applying, check that the bot has the
    required permissions and that its highest role is above every role it will
    edit. Report all problems together, before any change.
- **Name resolution:** accepts `#general`, `@Moderator`, a name, or an ID.
  Returns an error that lists the candidates when a name is ambiguous.

### 4.4 Discord adapter (`guildsmith-discord`)

- `twilight-http` client with its built-in rate limiter. Retries with jitter
  on 5xx and network errors. 429s are handled by the rate limiter; mutations
  are never retried blindly.
- Converts twilight models to and from core types, so twilight types never
  leave this crate.
- **Rate-limit budgeting.** Some endpoints (role creation, channel
  reordering) have strict or hidden limits. `plan` estimates how many calls an
  apply will make. `apply` reports progress so long runs don't look stuck.
- **Snapshot cache:** the snapshot is cached for the length of one MCP
  session and invalidated by our own writes. The `refresh` tool clears it.

### 4.5 MCP layer (`guildsmith-mcp`)

**Using few tokens is a design goal:**

- **Tool groups.** Only `core` is on by default. Others are enabled with
  `GUILDSMITH_TOOLSETS=core,moderation,…`.
- **Short tool descriptions.** One sentence plus the parameter docs. Longer
  guidance lives in the plugin's skills and is loaded only when needed.
- **Compact results.**
  - Results are short text, plus `structuredContent` / `outputSchema` where
    the client supports it.
  - List tools return only key fields by default; `fields=` asks for more.
    Long lists are paged with a cursor.
  - Never return raw Discord JSON.
- **Tool annotations:** each tool is marked `readOnlyHint` or
  `destructiveHint`, so clients can ask before running it.

**Draft v1 tool set (`core`, about 20 tools):**

| Tool | Purpose |
|---|---|
| `guilds_list` | Servers the bot can access, filtered by the allowlist |
| `guild_overview` | One compact summary: settings, counts, the role list and the channel tree |
| `refresh` | Clear the snapshot cache |
| `spec_export` | Live server → `guild.toml` text, optionally written to a path |
| `spec_validate` | Check a spec against the schema and its references, without network access |
| `spec_plan` | Spec → plan (a readable diff, the number of API calls, and risk) |
| `spec_apply` | Apply a plan, which must be the plan's hash from `spec_plan` (so the agent can't apply something nobody reviewed) |
| `roles_list` / `role_upsert` / `role_delete` | Roles |
| `channels_tree` / `channel_upsert` / `channel_delete` / `channels_reorder` | Channels and categories |
| `permissions_set` / `permissions_explain` | Permission overwrites; "why can't @X see #Y?" |
| `members_find` / `member_roles` | Search members; add or remove their roles |
| `message_send` / `messages_read` | Basic messaging, for announcements and checks |
| `bot_preflight` | Check the bot's permissions and role order for a planned action |

**Later tool groups:** `moderation` (bans, timeouts, prune), `automod`,
`events`, `forums`, `webhooks`, `emoji`, `onboarding`, `invites`, and
`messages` (the full set).

### 4.6 Spec example (`guild.toml`)

```toml
[guild]
name = "Guildsmith Lab"         # informational unless `manage_settings = true`

[[roles]]
name = "Moderator"
color = "#3BA55D"
hoist = true
permissions = ["kick_members", "manage_messages", "moderate_members"]

[[categories]]
name = "Community"

  [[categories.channels]]
  name = "general"
  kind = "text"
  topic = "Talk about anything"

  [[categories.channels]]
  name = "mod-log"
  kind = "text"
  overwrites = [
    { target = "@everyone", deny = ["view_channel"] },
    { target = "@Moderator", allow = ["view_channel"] },
  ]

[prune]
roles = false
channels = false
```

## 5. CLI

```
guildsmith mcp                      # run MCP server (stdio)
guildsmith export  -g <guild> -o guild.toml
guildsmith plan    -g <guild> guild.toml
guildsmith apply   -g <guild> guild.toml [--yes]
guildsmith doctor  -g <guild>       # token, allowlist, permissions, role hierarchy
guildsmith schema                   # print JSON Schema
```

The CLI and the MCP server call the same services in `guildsmith-app`.

## 6. Configuration and secrets

- **Precedence:** env vars, then `--config` file, then
  `~/.config/guildsmith/config.toml`.
- **Settings:**
  - `DISCORD_TOKEN`: required. Never logged, and never included in errors or
    tool output.
  - `GUILDSMITH_GUILDS`: the allowlist. Empty means *all guilds the bot is
    in*, with a warning at startup.
  - `GUILDSMITH_TOOLSETS`, `GUILDSMITH_READ_ONLY=1`, `GUILDSMITH_LOG`.
- **Optional:** store the token in the OS keyring (`keyring` crate) with
  `guildsmith auth set`, so it isn't kept in plain-text config.

## 7. Security model

- Supports only bot tokens with the least privilege needed. `doctor` lists the
  minimum permissions for each tool group.
- **Plan-hash gate:** `spec_apply` only applies a plan whose hash matches the
  output of `spec_plan`, and checks that the server hasn't changed in the
  meantime (snapshot fingerprint).
- Destructive live tools return a preview unless `confirm: true` is passed.
- Tool output wraps message content from users as data (fenced and labeled),
  so text in Discord messages is less likely to be read as instructions.
- `cargo deny` checks licenses and security advisories, and `cargo audit`
  runs in CI.

## 8. Quality: testing and reliability

| Layer | How it's tested |
|---|---|
| core | Unit tests and property tests (`proptest`): diff idempotence, op ordering, permission flag round-trips |
| app | Services tested against an in-memory `FakeDiscord` port (also used for demos) |
| discord | Recorded HTTP fixtures (`wiremock`) for mapping and error handling |
| mcp | Protocol tests: list tools, call each one, check results against their schemas, and measure the description sizes in tokens |
| e2e | Opt-in live tests against a dedicated test guild (`GUILDSMITH_LIVE_TEST=1`), run nightly in CI only |

- **CI:** fmt, clippy (`-D warnings`), tests, `cargo deny`, minimal-versions
  check, and release dry-run via cargo-dist.
- **Workspace lints:** `unsafe_code = "forbid"`, clippy pedantic (with
  documented allows).
- **Performance budget, checked in CI:**
  - The binary is under 10 MB stripped.
  - Cold start to the first `tools/list` response is under 50 ms.
  - The default tool descriptions together stay under a fixed token limit
    (target about 3k).

## 9. Distribution and integrations

| Channel | Plan |
|---|---|
| GitHub Releases | `cargo-dist` builds for linux-x64/arm64 (musl), macOS x64/arm64 and windows-x64, with checksums and an installer script |
| crates.io | Publish `guildsmith` and the library crates. Reserve all crate names early. |
| npm | `guildsmith` launcher, plus `@guildsmith/<platform>` packages declared as `optionalDependencies` (the esbuild/biome pattern), so `npx -y guildsmith mcp` works without Rust |
| Claude Code | A plugin with `.mcp.json` (runs `guildsmith mcp`), skills (spec authoring, safe apply workflow, permission debugging) and commands (`/guild-plan`, `/guild-apply`, `/guild-doctor`). Shipped from a marketplace repo (`guildsmith-marketplace` or the `plugins/` folder in this repo). |
| Codex | A `~/.codex/config.toml` `[mcp_servers.guildsmith]` snippet plus the same skills. **To check:** whether Codex has a plugin/package format now; if so, ship a manifest. |

## 10. Roadmap

| Milestone | Scope | Done when |
|---|---|---|
| **M0 Scaffold** | Workspace, CI, lints, cargo-dist config, reserve names on crates.io/npm/GitHub | CI passes on an empty skeleton |
| **M1 Read path** | Core types, twilight adapter for reads, `guilds_list`, `guild_overview`, `roles_list`, `channels_tree`, `spec_export`, `doctor` | Exporting a real server produces valid TOML that re-imports correctly |
| **M2 Plan/apply** | Diff, planner, ordering, `spec_validate/plan/apply`, plan-hash gate, preflight | Live test: apply an example spec to an empty server, then plan shows no changes |
| **M3 Live tools + safety** | Remaining `core` tools, allowlist, read-only mode, confirm previews, audit reasons | MCP protocol tests pass; token budget met |
| **M4 Ship** | npm launcher, Claude Code plugin + marketplace, Codex docs, examples, docs site | Fresh install works through each channel on Linux, macOS and Windows |
| **M5 Later** | More tool groups, streamable HTTP transport, optional gateway (`twilight-gateway`) for events and drift watching, server templates import | Driven by user demand |

## 11. Open questions

1. One repo with `plugins/` inside, or a separate marketplace repo?
   Recommendation: start in one repo and split later if needed.
2. Should `spec_apply` from MCP *require* a reviewed plan hash, or only when
   the plan includes destructive ops? Recommendation: always require it in v1.
3. Should the spec cover onboarding, automod and server settings in v1, or
   only structure (roles, channels, overwrites)? Recommendation: structure
   only.
4. What is the minimum supported Rust version? Recommendation: latest stable
   minus 2.
5. Codex plugin format (see §9).

## 12. Research references (2026-10-19)

- **No first-party Discord MCP.** Discord offers only a docs-search MCP at
  `https://docs.discord.com/mcp` (added 2026-03-09).
- **Bots can't create servers.** Discord's change log, "Guild Create
  Deprecation", 2025-07-28.
- **Existing community servers reviewed:**
  - PaSympa/discord-mcp: 99 tools, allowlist, dry-run, active.
  - HardHeadHackerHead/discord-mcp (`@quadslab.io/discord-mcp`): about 139
    tools, last updated 2026-03.
  - barryyip0625/mcp-discord: active.
  - v-3/discordmcp: stale since 2025-01.
- **Terraform-style precedent:** Lucky3028/terraform-provider-discord (now
  archived). It confirms demand for server-as-code.
