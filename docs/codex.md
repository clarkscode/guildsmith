# Using guildsmith with Codex

guildsmith runs as an MCP server over stdio (`guildsmith mcp`). Codex can load it as a plugin or
from a plain `config.toml` entry.

## 1. Set the token

Codex has no prompt for secrets, so set the bot token (and optionally the allowlist) in the
environment Codex starts from, for example in your shell profile:

```bash
export DISCORD_TOKEN=…                          # the bot token from the Discord Developer Portal
export GUILDSMITH_GUILDS=123456789012345678     # optional: comma-separated server ids
```

Both setups below forward these variables with `env_vars`, so the token never appears in a config
file.

## 2a. Install the plugin

```bash
codex plugin marketplace add clarkscode/guildsmith
```

Then install `guildsmith` from that marketplace. The plugin (`plugins/codex/`) runs
`npx -y guildsmith@<version> mcp` and adds the guild-* skills: writing `guild.toml`, applying
safely, debugging permissions, plus plan, apply and doctor workflows.

## 2b. Or configure it by hand

Add this to `~/.codex/config.toml`:

```toml
[mcp_servers.guildsmith]
command = "guildsmith"          # or "npx" with args = ["-y", "guildsmith@<version>", "mcp"]
args = ["mcp"]
env_vars = ["DISCORD_TOKEN", "GUILDSMITH_GUILDS"]
default_tools_approval_mode = "writes"
```

`default_tools_approval_mode = "writes"` lets read-only tools run without asking and asks before
any tool that writes. Every tool carries accurate `readOnlyHint` / `destructiveHint` annotations,
which this mode relies on.

Install the `guildsmith` binary with the shell or PowerShell installer from the GitHub release,
`cargo install guildsmith`, or use the `npx` form above.

## 3. Check it works

Ask Codex to "list my Discord servers with guildsmith", or run `guildsmith doctor -g <server id>`
in a terminal. Optional tool groups are enabled with `GUILDSMITH_TOOLSETS=core,moderation`; add it
to `env_vars` to forward it.
