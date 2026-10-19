# Release checklist

## One-time, owner only (manual)

These steps publish names publicly and can't be undone. They are done by the repository owner,
never by CI or an automated task.

- [ ] Reserve on crates.io: `guildsmith`, `guildsmith-core`, `guildsmith-app`,
      `guildsmith-discord`, `guildsmith-mcp`.
- [ ] Reserve on npm: `guildsmith` and the `@guildsmith` scope.
- [ ] Create the GitHub environment `release` with required reviewers. The npm and crates.io
      publish jobs wait for its approval.
- [ ] Add the repository secrets `CARGO_REGISTRY_TOKEN` and `NPM_TOKEN` to the `release`
      environment.

## Each release

1. Update `version` in the root `Cargo.toml` and the plugin manifests.
2. Run `cargo xtask budgets` and the quickstart scenarios (`specs/*/quickstart.md`).
3. Tag `vX.Y.Z` and push the tag. The release workflow builds the binaries with dist.
4. Approve the `release` environment to publish to npm and crates.io.
