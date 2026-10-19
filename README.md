# guildsmith

Fast MCP server and CLI for building and managing Discord servers, with plugins for Claude Code
and Codex.

guildsmith is one static Rust binary with two ways to work:

1. **Live tools**: a small set of typed MCP tools for day-to-day operations.
2. **Server as code**: `guild.toml` describes a server's roles, categories, channels and
   permissions. guildsmith can **export** an existing server to that file, **plan** the changes
   as a reviewable diff, and **apply** them. Running `plan` after `apply` shows no changes.

It is safe by default: a server allowlist, read-only mode, previews before destructive actions,
a reviewed-plan check before apply, and an audit-log reason on every change.

> Status: early development. Nothing is released yet.

## Install

Not published yet. Planned channels: prebuilt binaries, `npx -y guildsmith mcp`,
`cargo install guildsmith`, the Claude Code plugin, and the Codex plugin.

## Documentation

See [`docs/`](docs/) and the design documents in [`specs/`](specs/).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
