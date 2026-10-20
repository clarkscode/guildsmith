# guildsmith

**Manage Discord servers from your AI agent, or from the command line.** guildsmith is an MCP
server and CLI in one small Rust binary, with ready-made plugins for Claude Code and Codex.

- **Day-to-day admin by asking**: "ban these raiders", "make an #announcements channel only mods
  can post in", "schedule game night every Friday".
- **Your whole server as code**: export a server to a `guild.toml` file, review the changes as
  a plan, apply them, and copy one server's setup to another.
- **Safe by default**: destructive actions show a preview until you confirm, a server allowlist,
  a read-only mode, and an audit-log reason on every change.

> **Status:** early development, not published yet. Until the first release, build it from
> source ([below](#build-from-source)).

## Contents

1. [Quick start](#quick-start)
2. [What it can do](#what-it-can-do)
3. [Server as code](#server-as-code)
4. [Configuration](#configuration)
5. [Safety](#safety)
6. [Documentation](#documentation)

## Quick start

Setup takes about five minutes: create a bot, install guildsmith, connect your agent.

### 1. Create a Discord bot

1. Open the [Discord Developer Portal](https://discord.com/developers/applications) and click
   **New Application**.
2. In **Bot**, click **Reset Token** and copy the token. Keep it secret: anyone with it controls
   the bot.
3. Optional: in **Bot → Privileged Gateway Intents**, turn on **Message Content Intent**. Only
   the `messages` tools (search, reading message text) need it.
4. In **OAuth2 → URL Generator**, tick the `bot` scope and the **Administrator** permission,
   open the generated URL, and add the bot to your server.

   Administrator is needed to apply a spec file, because it is the only way the bot can see
   every channel ([why](docs/adr/001-administrator-for-spec-apply.md)). For live tools only, you
   can grant fewer permissions; `guildsmith doctor` lists what is missing.
5. Copy your server's id: in Discord, turn on **Settings → Advanced → Developer Mode**, then
   right-click the server icon and choose **Copy Server ID**.

### 2. Connect your agent

Pick the agent you use. Each one asks for the bot token once and keeps it out of config files.

#### Claude Code

In Claude Code, run:

```
/plugin marketplace add clarkscode/guildsmith
/plugin install guildsmith@guildsmith
```

Claude Code asks for the bot token (stored in your OS secure store) and the server ids to allow.
The plugin adds the MCP server and skills that teach Claude the safe workflows.

#### Codex

Set the token in the environment Codex starts from (for example in your shell profile), then add
the plugin:

```bash
export DISCORD_TOKEN=…                        # the bot token
export GUILDSMITH_GUILDS=123456789012345678   # your server id(s), comma-separated
codex plugin marketplace add clarkscode/guildsmith
```

Then install `guildsmith` from that marketplace. Details: [docs/codex.md](docs/codex.md).

#### Any other MCP client (Cursor, Claude Desktop, …)

Store the token once in your OS keyring, then point the client at `guildsmith mcp`:

```bash
guildsmith auth set          # paste the token at the hidden prompt
```

```json
{
  "mcpServers": {
    "guildsmith": {
      "command": "guildsmith",
      "args": ["mcp"],
      "env": { "GUILDSMITH_GUILDS": "123456789012345678" }
    }
  }
}
```

Without the binary installed, use `"command": "npx", "args": ["-y", "guildsmith", "mcp"]`.

#### No agent: the CLI

```bash
guildsmith auth set
guildsmith doctor -g <server id>              # checks the token, permissions and role order
guildsmith export -g <server id> -o guild.toml
```

### 3. Check it works

Ask your agent: **"List my Discord servers with guildsmith"**, or run
`guildsmith doctor -g <server id>`. Doctor reports every problem with its fix.

Then try:

- "Show me the channel tree of my server."
- "Why can't @Member post in #announcements?"
- "Export my server to guild.toml and explain what's in it."
- "Time out the member spamming #general for an hour."

## What it can do

Agents start with the **core** tools (23) and switch on other groups when a task needs them, so
you don't have to configure anything. To register groups up front, set `GUILDSMITH_TOOLSETS`.

| Group | What it covers |
|---|---|
| `core` (always on) | Servers, roles, channels, permissions and why they apply, members, sending and reading messages, spec export/plan/apply |
| `moderation` | Bans (in bulk), timeouts, kicks, purging messages, pruning, raid lockdown, audit log |
| `messages` | Rich messages and embeds, editing, pins, reactions, crossposting, search |
| `threads` | Threads and forum posts, forum tags |
| `polls` | Polls and their results |
| `events` | Scheduled events, with time zones and repeats |
| `invites` | Invites, with roles and expiry |
| `voice` | Moving, muting and disconnecting members, stage instances, voice channel status |
| `automod` | Auto-moderation rules |
| `webhooks` | Webhooks; a new webhook's URL is shown only once |
| `expressions` | Custom emoji, stickers and soundboard sounds |
| `settings` | Server settings, onboarding, welcome screen, templates, integrations |
| `raw` (off unless enabled) | Any Discord API request the other tools don't cover |

Every tool is listed in [docs/tools.md](docs/tools.md). Each CLI command maps to a tool:
`guildsmith <group> <tool> --flags`, for example
`guildsmith moderation ban --members spammer --delete-message-seconds 1d`.

## Server as code

A `guild.toml` file describes a server: roles, categories, channels and permissions, and
optionally its settings, automod rules, onboarding, emoji, webhooks, invites, events and pinned
rules posts.

```toml
[[roles]]
name = "Moderator"
color = "#3BA55D"
permissions = ["kick_members", "moderate_members", "manage_messages"]

[[categories]]
name = "Community"

  [[categories.channels]]
  name = "announcements"
  kind = "announcement"
  overwrites = [{ target = "@everyone", deny = ["send_messages"] }]
```

```bash
guildsmith export -g <server> -o guild.toml   # current server → file (files go to assets/)
guildsmith plan   -g <server> guild.toml      # what would change, and a plan hash
guildsmith apply  -g <server> guild.toml      # apply after you confirm the plan
```

- Anything you leave out is left alone; nothing is deleted unless you turn on `[prune]`.
- Running `plan` after `apply` shows no changes.
- To copy a server, export one and apply the file to another.

Ready-made examples: [`examples/`](examples) (a community, a dev team, a gaming server, and
[`full-server`](examples/full-server) with one of everything). Full reference:
[docs/spec-reference.md](docs/spec-reference.md). Editors that support `#:schema` (Taplo, Even
Better TOML) autocomplete the file.

## Configuration

All settings are environment variables. Only the token is required.

| Variable | Default | Purpose |
|---|---|---|
| `DISCORD_TOKEN` | — | The bot token. `guildsmith auth set` stores it in the OS keyring instead. |
| `GUILDSMITH_GUILDS` | all servers the bot is in | Comma-separated server ids guildsmith may touch. Set it. |
| `GUILDSMITH_READ_ONLY` | off | `1` refuses every change. |
| `GUILDSMITH_TOOLSETS` | `core` | Groups to register at startup, e.g. `core,moderation`, or `all`. |
| `GUILDSMITH_TOOLSETS_BLOCK` | none | Groups that can never be used, even if an agent asks, e.g. `raw`. |
| `GUILDSMITH_FILE_ROOTS` | none | Directories MCP tools may read and write files in (emoji images, exports). |
| `GUILDSMITH_LOG` | `warn` | Log level, written to stderr. The token is always scrubbed. |

## Safety

guildsmith gives an AI agent admin power over a real community, so the guardrails don't depend
on the agent behaving:

- **Previews first.** Bans, deletes, purges and other destructive actions return what they
  would do, and act only when called again with `confirm`.
- **Reviewed plans only.** `apply` needs the hash of a plan you saw, and refuses if the server
  changed since.
- **Boundaries.** Servers outside `GUILDSMITH_GUILDS` and every change in read-only mode are
  refused.
- **Accountability.** Every change carries an audit-log reason (`guildsmith: …`).
- **No leaked secrets.** The bot token never appears in output or logs, webhook URLs are shown
  once, and message text written by members is marked as untrusted data for the agent.

## Documentation

| Read this | For |
|---|---|
| [docs/tools.md](docs/tools.md) | Every tool, its inputs and the permissions it needs |
| [docs/spec-reference.md](docs/spec-reference.md) | Every key of `guild.toml` |
| [docs/codex.md](docs/codex.md) | Codex setup in detail |
| [docs/architecture.md](docs/architecture.md) | How the code is organized |
| [docs/adr/](docs/adr) | Design decisions |
| [docs/release.md](docs/release.md) | Releasing (maintainers) |

### Build from source

Needs Rust 1.97 or newer.

```bash
git clone https://github.com/clarkscode/guildsmith
cd guildsmith
cargo install --path crates/guildsmith      # installs the `guildsmith` binary
```

Then follow the [Quick start](#quick-start), using the "any other MCP client" or CLI steps. The
plugins need the published npm package and work after the first release.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
