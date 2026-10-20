# Releasing Guildsmith

Run `python3 scripts/release.py --help` from this checkout. Requires Python 3.11+,
Rust/Cargo, Node/npm, GitHub CLI (`gh`) and cargo-dist 0.32.0.
On Windows use `py -3 scripts/release.py`.

Without a subcommand the helper runs local checks. Remote write commands are previews
until you add `--execute`. It never approves a GitHub environment, overwrites a release
tag or stores credentials.

## Release channels

| Channel | How it is released |
| --- | --- |
| GitHub binaries | A version tag triggers five platform builds, installers and SHA-256 files |
| crates.io | GitHub publishes core → app → discord → mcp → CLI after `release` approval |
| npm | First release uses local login and 2FA; later releases use GitHub Trusted Publishing |
| Codex | Repository marketplace points to `plugins/codex`, pinned to the npm version |
| Claude Code | Repository marketplace points to `plugins/claude-code`, pinned to the npm version |

The two repository marketplaces do not have a separate upload command. Pushing their
files distributes the definitions; they become usable when the pinned npm version exists.
This does not submit plugins to a curated directory or verify client installation.
Test installation in both clients after publication.

## One-time setup

- Sign into crates.io, verify your email and check the five crate names are available or yours.
- Create an npm account with 2FA and the `@guildsmith` organization. Check the unscoped
  `guildsmith` package name too. Claim names by publishing real packages, not empty placeholders.
- Create GitHub's `release` environment with required reviewers. Allow self-review if
  you are the sole maintainer.
- Add `CARGO_REGISTRY_TOKEN` as an **environment secret**, with `publish-new` and
  `publish-update` limited to the five crate names.
- Use `npm login` locally for the first npm publication. No `NPM_TOKEN` secret is required.

## Prepare and check

For a new stable version, start with a clean working tree:

```bash
python3 scripts/release.py version 0.1.1
```

Choose the intended version. This updates the workspace version, internal dependencies,
workspace lockfile entries, six npm manifests, both plugin manifests and their MCP version
pins. External dependencies and authentication settings are preserved. Prereleases are
deliberately unsupported. Update `CHANGELOG.md` yourself, then run:

```bash
python3 scripts/release.py check
```

This checks formatting, lints, tests, plugin manifests and skill copies, npm versions and
launcher syntax, schema drift, the actual `dist` build profile and performance limits,
generated CI and the release plan. CI also checks other operating systems, minimum Rust
and dependencies. For the initial version, skip the version command unless choosing a
new patch version to recover from a failed tag.

Live Discord tests return early unless explicitly enabled. Review `specs/*/quickstart.md`
and use dedicated test servers for live verification. A normal test pass does not prove
live Discord testing.

## Commit, push and tag

Review and commit the changes, push `main`, and wait for its CI run to pass. Then:

```bash
python3 scripts/release.py tag
python3 scripts/release.py tag --execute
gh run list --workflow release.yml --limit 5
```

Execution requires a clean checkout matching remote `main`, successful push CI for that
exact commit, synchronized versions and no existing local or remote tag. It creates and
pushes only the annotated version tag. Use the **numeric run ID**, not a commit hash, with
`gh run watch`.

The GitHub Release is created before crates.io publication waits for approval. Review
the build and approve `release` in GitHub to publish the crates.

For failure details use `gh run view <numeric-run-id> --log-failed`. Rerunning an old
workflow uses its old commit, not fixes on `main`. The helper refuses existing tags;
prefer a new patch version for retries. Moving an unpublished failed tag is a separate,
explicit owner decision after checking release assets and registries. Never overwrite
published versions. If a tag push fails after local creation, inspect the remote before
retrying the push; the helper will not delete that local tag.

## First npm publication

The first-release configuration excludes `./publish-npm` from `dist-workspace.toml`.
Its npm installation checks run only when manually dispatched.

Check out the successful release tag to match npm templates with the binaries:

```bash
git switch --detach v0.1.0
npm login
python3 scripts/release.py npm-stage
```

Use your actual version. The helper verifies HEAD against the local and remote tag,
downloads the five archives and individual SHA-256 files, verifies their checksums,
and stages packages using `cargo xtask npm-pack`. It creates six npm tarballs in a new
directory under `target/release-helper/`. Inspect their contents before publication.

```bash
python3 scripts/release.py npm-publish
python3 scripts/release.py npm-publish --execute
```

Execution downloads and verifies the artifacts again, packages the tagged source, then
publishes the platform packages before the launcher. Complete npm's interactive 2FA
prompts. Retries skip existing versions; network/authentication errors stop publication.
Manual publication does not request CI provenance.

```bash
python3 scripts/release.py smoke --execute
gh run list --workflow install-smoke.yml --limit 5
```

Verify the installation checks and both agent plugins. The existing cargo-install smoke
job installs tagged source; additionally test the registry with:

```bash
cargo install guildsmith --version 0.1.0 --locked --root /tmp/guildsmith-registry-check
```

Use the actual version and an appropriate temporary directory on your OS.
Return to `main` with `git switch main`.

## Enable Trusted Publishing

The reusable `publish-npm.yml` is prepared for OIDC using Node 24, npm >= 11.5.1,
`id-token: write` and no `NPM_TOKEN`. It remains disconnected until setup is complete.

On npm, open **Settings → Trusted publishing** on all six packages: `guildsmith`,
`@guildsmith/linux-x64`, `@guildsmith/linux-arm64`, `@guildsmith/darwin-x64`,
`@guildsmith/darwin-arm64` and `@guildsmith/win32-x64`.

Choose GitHub Actions:

| Field | Value |
| --- | --- |
| Organization or user | `clarkscode` |
| Repository | `guildsmith` |
| Workflow filename | `release.yml` |
| Environment | `release` |
| Allowed actions | Allow direct publishing with `npm publish` |

npm validates the **calling workflow**, `release.yml`, even though the publish command
lives in `publish-npm.yml`. See [npm's guide](https://docs.npmjs.com/trusted-publishers/)
and its [reusable-workflow troubleshooting](https://github.com/npm/documentation/blob/main/content/packages-and-modules/securing-your-code/trusted-publishers.mdx#troubleshooting).

Configure this when the next release is ready: npm currently requires the first successful
publication within two days of creating the trust configuration. An expired unused
configuration must be deleted and recreated.

Then, on `main`:

1. Restore `publish-jobs = ["./publish-npm", "./publish-crates"]` in `dist-workspace.toml`.
2. Remove the temporary `if: github.event_name == 'workflow_dispatch'` and its comment
   from the npm job in `.github/workflows/install-smoke.yml`.
3. Run `dist generate --mode ci`.
4. Prepare the next version, check, commit, push, wait for CI and tag as above.
5. Verify OIDC publication of all six packages and the installation checks.

Keep the `release` approval gate and npm account 2FA. There is no npm token to renew.
The crates.io token is separate: renew it before expiry unless independently migrating
crates.io to Trusted Publishing.
