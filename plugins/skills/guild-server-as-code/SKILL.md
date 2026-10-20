---
name: guild-server-as-code
description: How to manage a whole Discord server as a guildsmith spec file - settings, automod, onboarding, welcome screen, emoji, stickers, sounds, webhooks, invites, events and fixed messages - with export, plan, apply, ids written back, webhook URLs shown once and items that expire. Use when the user wants to copy, back up, review or rebuild a server beyond its roles and channels.
user-invocable: false
---

# Server as code with guildsmith

A spec file (`guild.toml`) can describe much more than roles and channels. Every section is
optional; a section that's left out is not managed. `docs/spec-reference.md` has every key, and
`examples/full-server/` has one item of every kind.

## Export first

`spec_export` (or `guildsmith export -g <server> -o dir/guild.toml`) writes every kind the bot
can read. With `assets_dir` (MCP, inside `GUILDSMITH_FILE_ROOTS`) or `-o` (CLI), emoji, stickers,
sounds and images are saved under `assets/<kind>/` and referenced as `{ file = "…" }`;
otherwise they stay CDN `url`s. Fixed messages aren't exported: add the bot's posts that should
stay as written by hand. Webhook URLs and tokens are never exported.

## Plan, review, apply

Same as structure changes: `spec_plan`, show the user every op (destructive ones first), then
`spec_apply` with the plan hash after they approve. Things to point out in the plan:

- **Replaces**: a changed emoji, sticker or sound file, or an automod rule whose trigger type
  changes, is deleted and created again. Old messages stop showing a replaced emoji.
- **Settings**: turning Community off or Discovery on is destructive. Turning Community on needs
  a rules channel, a public updates channel, verification `low`+ and the content filter on
  `all_members`; onboarding and the welcome screen need Community.
- **Warnings** about items guildsmith won't recreate (below) are part of the review.

## Ids written back

Items without an `id` (`code` for invites) are matched by name, by signature (invites) or by
exact content (messages). After apply, their ids are written into the spec file: the CLI does it
by default (`--no-write-ids` to skip), `spec_apply` does it when given `path`. Only `id = "…"`
and `code = "…"` lines are added; tell the user the file changed so they can commit it. A second
apply writes nothing.

## Secrets: shown once

A webhook created by apply appears once in the result's `secrets` as `{webhook, url}`. Give the
URL to the user right away and say it isn't shown again; never put it in the spec file (validation
rejects anything shaped like a webhook URL or token), a commit, a log or another tool call. The CLI
leaves it out of `--json` unless `--show-secrets` is passed.

## Things that expire

- **Events** that have started change only their description and end; completed, canceled and
  past events are reported, never made again. Remove them from the file when they're over.
- **Invites** can't be edited. One that expired or was deleted is made again only with
  `recreate = true`; otherwise the plan warns.
- **Fixed messages** are only ever the bot's own: guildsmith never edits someone else's post.

## Copying a server

Export server A, then plan the file against server B. The file's `[guild] id` names A, so its
ids are hints: anything missing on B is created there, and B's ids are written back. Review the
plan carefully before applying to a server that already has content.
