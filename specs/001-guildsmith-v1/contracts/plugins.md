# Contract: agent integrations

Layout and fields follow R15 in [../research.md](../research.md). Versions in snippets are
placeholders that the release job fills in.

## Claude Code

The repo root holds `.claude-plugin/marketplace.json`:
```json
{
  "name": "guildsmith",
  "owner": { "name": "clarkscode" },
  "plugins": [{ "name": "guildsmith", "source": "./plugins/claude-code" }]
}
```

`plugins/claude-code/.claude-plugin/plugin.json` sets `name`, `version`, `description`,
`license`, `repository`, and:
```json
"userConfig": {
  "discord_token": { "type": "string", "title": "Discord bot token", "sensitive": true, "required": true },
  "guilds": { "type": "string", "title": "Allowed server IDs (comma-separated)", "required": false }
}
```

`plugins/claude-code/.mcp.json`:
```json
{ "mcpServers": { "guildsmith": {
  "command": "npx", "args": ["-y", "guildsmith@<version>", "mcp"],
  "env": { "DISCORD_TOKEN": "${user_config.discord_token}", "GUILDSMITH_GUILDS": "${user_config.guilds}" }
} } }
```

Skills, under `plugins/claude-code/skills/` (copied from `plugins/skills/`):

| Skill | User-invocable | Purpose |
|---|---|---|
| `guild-spec` | no | How to write and edit `guild.toml` |
| `guild-safe-apply` | no | export → edit → validate → plan → show the user → apply with the hash |
| `guild-permissions` | no | Debug "can't see or post" with `permissions_explain` |
| `guild-plan` | yes (`/guild-plan`) | Plan a file against a server and summarize it |
| `guild-apply` | yes (`/guild-apply`) | Apply a plan the user has reviewed |
| `guild-doctor` | yes (`/guild-doctor`) | Run `bot_preflight` and explain how to fix problems |

Install:
```
/plugin marketplace add clarkscode/guildsmith
/plugin install guildsmith@guildsmith
```

## Codex

Plugin: `plugins/codex/.codex-plugin/plugin.json`, `plugins/codex/.mcp.json` (same command, with
`"env_vars": ["DISCORD_TOKEN", "GUILDSMITH_GUILDS"]`), and `plugins/codex/skills/` (the same
skills). Marketplace file at the repo root: `.agents/plugins/marketplace.json`.
```
codex plugin marketplace add clarkscode/guildsmith
```

The plain-config alternative, documented in `docs/codex.md`:
```toml
[mcp_servers.guildsmith]
command = "guildsmith"          # or "npx" with args = ["-y", "guildsmith@<version>", "mcp"]
args = ["mcp"]
env_vars = ["DISCORD_TOKEN", "GUILDSMITH_GUILDS"]
default_tools_approval_mode = "writes"
```

## npm

- `guildsmith` contains `bin/guildsmith.js`. It finds `@guildsmith/<platform>` with
  `require.resolve`, then runs that binary with the same arguments and stdio, and passes through
  its exit code. It has no postinstall script.
- `@guildsmith/{linux-x64, linux-arm64, darwin-x64, darwin-arm64, win32-x64}` each contain one
  binary and set only `os` and `cpu`. The Linux binaries are static musl builds, so they run on
  both glibc and musl systems. Setting `libc` would stop them installing on glibc.
