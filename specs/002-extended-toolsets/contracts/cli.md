# Contract: CLI additions

Extends [001 cli.md](../../001-guildsmith-v1/contracts/cli.md). Exit codes and global options
are unchanged.

## Configuration

| Env var | Meaning |
|---|---|
| `GUILDSMITH_TOOLSETS` | Groups registered at startup: a comma list, `all`, or `default` (unchanged from v1, now with the new names). |
| `GUILDSMITH_TOOLSETS_BLOCK` | Groups the agent may not enable (FR-003b). Takes precedence over `GUILDSMITH_TOOLSETS`. |
| `GUILDSMITH_FILE_ROOTS` | Directories that MCP tools may read assets from (E7). Empty by default. |

## Commands

Every tool in [mcp-tools.md](mcp-tools.md) is also available as
`guildsmith <group> <tool-name-without-prefix> [--flags]`, using the same services and policy
(FR-008), for example:
```
guildsmith moderation ban -g Lab --members @a,@b --delete-messages 1d          # previews
guildsmith moderation ban -g Lab --members @a,@b --delete-messages 1d --yes
guildsmith messages purge -g Lab '#general' --author @spammer --count 50 --yes
guildsmith events create -g Lab --name "Game night" --start 2026-10-23T20:00-07:00 --voice Lobby
guildsmith settings template-import <code> -o imported.toml
guildsmith raw GET guilds/{guild}/audit-logs -g Lab
guildsmith raw PATCH guilds/{guild} -g Lab --body '{"description":"…"}' --yes
```

- `--yes` stands in for `confirm: true`. Without it on a TTY the CLI shows the preview and asks.
  On a non-TTY it prints the preview and exits `3`.
- `{guild}` in raw paths is replaced with the resolved `-g` id.
- `guildsmith toolsets` lists every group with its tool count, approximate tokens and the
  permissions it needs (FR-010).
- `guildsmith doctor` now also checks the permissions and intents of every enabled group.

## `apply` changes

- `--write-ids` / `--no-write-ids` (default: on when given a file path). Writes created ids into
  the file (E9).
- `--show-secrets`: includes newly created webhook URLs in `--json` output. Without it, they go
  only to the terminal (stderr) and are never written to a file or log (E8).
