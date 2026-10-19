# ADR-002: Our own npm per-platform packages

- Status: accepted
- Date: 2026-10-19
- Context: research R15

## Context

cargo-dist's npm installer is a single package whose `postinstall` script downloads the binary
from GitHub Releases. That fails with `npm install --ignore-scripts`, under pnpm 10's default
script blocking, and offline.

## Decision

- Publish `guildsmith` (a small launcher) plus `@guildsmith/{linux-x64,linux-arm64,darwin-x64,
  darwin-arm64,win32-x64}`, listed as `optionalDependencies` (the esbuild/biome pattern).
- Each platform package sets only `os` and `cpu`. The Linux binaries are static musl builds that
  run on glibc too, so `libc` is deliberately left unset: npm would skip a `libc: musl` package
  on glibc systems.
- A CI job builds these packages from the dist artifacts (`cargo xtask npm-pack`).

## Consequences

- No install scripts run, so `npx -y guildsmith mcp` works everywhere npm does.
- We maintain the packaging job ourselves instead of relying on dist's installer.
