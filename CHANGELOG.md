# Changelog

All notable changes to guildsmith are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Changes to the default tool set, the spec file
format or the safety policy are always called out.

## 0.1.1

### Fixed

- Define the build profile required by cargo-dist.

### Added

- Release helper for version synchronization, validation and publication.
- npm Trusted Publishing workflow, ready to enable after initial publication.

## 0.1.0

The first release: an MCP server and CLI for managing Discord servers, with plugins for Claude
Code and Codex.

### Added

**Live tools for AI agents**
- 23 default tools: servers, roles, channels, permission overwrites and a step-by-step
  permission explainer, members, sending and reading messages, and spec export/plan/apply.
- 12 opt-in tool groups (84 tools in all): `moderation`, `messages`, `threads`, `polls`,
  `events`, `invites`, `voice`, `automod`, `webhooks`, `expressions`, `settings` and `raw`.
- Agents switch groups on themselves through the `toolsets` and `tool_call` tools; the tool list
  never changes mid-session. Operators can register groups up front (`GUILDSMITH_TOOLSETS`) or
  rule them out (`GUILDSMITH_TOOLSETS_BLOCK`).
- Bulk moderation: ban up to 200 accounts per request, purge, prune and raid lockdown, with
  progress, cancellation and a report of what succeeded, failed or wasn't attempted.

**Server as code**
- `guild.toml` describes a server: roles, categories, channels and permissions, plus settings,
  automod rules, onboarding, the welcome screen, emoji, stickers, sounds, webhooks, invites,
  scheduled events and fixed messages such as a rules post.
- `export` writes a server to a file (with its images and sounds under `assets/`), `validate`
  checks it offline, `plan` shows the changes with a plan hash, and `apply` makes them. Planning
  after applying shows no changes.
- `apply` writes the ids of new items back into the file, adding only `id`/`code` lines.
- A JSON Schema for editor autocompletion, and example files including `examples/full-server`.

**CLI**
- `guildsmith export | validate | plan | apply | doctor | auth | schema | toolsets`, and every
  tool as `guildsmith <group> <tool> --flags`.
- `guildsmith doctor` checks the token, permissions, role order and intents, and says how to fix
  each problem.
- `guildsmith auth set` stores the bot token in the OS keyring.

**Plugins**
- Claude Code and Codex plugins with skills for safe applies, permissions, moderation, community
  tools, server as code and raw requests.

### Safety
- Destructive actions return a preview until confirmed; `apply` needs the hash of a reviewed
  plan and refuses if the server changed since.
- A server allowlist, a read-only mode, and an audit-log reason on every change.
- The bot token never appears in output or logs; webhook URLs are shown once and never stored;
  text written by server members is marked as untrusted data for the agent.
- The opt-in `raw` group can send any Discord API request under the same rules, and is the only
  tool allowed to return Discord's JSON (constitution 1.1.0, ADR-005).

### Install
- Prebuilt binaries for Linux, macOS and Windows with shell and PowerShell installers,
  `npx -y guildsmith mcp`, and `cargo install guildsmith`.
