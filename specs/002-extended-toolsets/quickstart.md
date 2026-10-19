# Quickstart: validating extended toolsets

Prerequisites are the same as [001 quickstart](../001-guildsmith-v1/quickstart.md), plus:
- **Two** test servers (`$LAB` with content, `$EMPTY` freshly created), both on the allowlist.
- The bot has Administrator. The **MESSAGE_CONTENT** privileged intent is turned on in the
  developer portal (needed for search, E11).
- A few test accounts (alts) that can join `$LAB`, for moderation scenarios.

Contracts: [mcp-tools.md](contracts/mcp-tools.md) · [spec-format.md](contracts/spec-format.md) ·
[cli.md](contracts/cli.md).

## 0. Offline

```bash
cargo test --workspace            # FakeDiscord convergence for every kind, raw classifier, BulkOutcome invariant
cargo xtask budgets               # default ≤ 25 tools / 3k tokens; each group ≤ 15 / 1.5k; all ≤ 15k
```

## 1. Tool groups and dispatch (US8, SC-002/003/013)

1. Start `guildsmith mcp` with no `GUILDSMITH_TOOLSETS`. **Expect** `tools/list` to show 23
   tools.
2. Call `toolsets {action: "list"}`. **Expect** 12 optional groups with tool counts, tokens and
   permissions.
3. Call `toolsets {action: "enable", toolset: "polls"}`, then
   `tool_call {tool: "poll_create", arguments: {...}}`. **Expect** the poll to appear in Discord
   with no restart, `tools/list` to be unchanged, and stderr to log the enablement.
4. Restart with `GUILDSMITH_TOOLSETS_BLOCK=raw` and try to enable `raw`. **Expect** a refusal.
5. Restart with `GUILDSMITH_TOOLSETS=all`. **Expect** 84 tools, and the startup log to report
   the approximate token cost.

## 2. Moderation (US1, SC-004/005/008)

- `member_ban` with 3 alts and no confirm. **Expect** a preview naming them, with nothing banned.
- Run it again with `confirm`. **Expect** them banned, a report of succeeded/failed/not_attempted
  that adds up to 3, and audit-log reasons.
- `messages_purge` in a channel holding one 15-day-old message. **Expect** the old message to be
  reported separately and deleted only with `include_old`.
- `members_prune {days: 30}` without confirm. **Expect** a count only.
- Time a 50-id ban list from asking to the final report. **Expect** under 2 minutes (SC-008;
  fixture ids are fine in the FakeDiscord run).

## 3. Content, community and server areas (US2–US7)

Run each user story's acceptance scenarios from [spec.md](spec.md) through `tool_call`, and
again through the CLI (`guildsmith <group> …`), so both paths are covered (FR-008). Points to
check:
- Forum post with an unknown tag: a refusal that lists the valid tags.
- `webhook_create`: the URL appears once. `webhooks_list` and the logs contain no URL (SC-006).
- `voice_members` naming a member who isn't in voice: that member is reported, and the others
  are moved.
- `template_import` of a public template: the output passes `spec_validate` (SC-009).
- `event_create` without a time zone: UTC, shown as "UTC".

## 4. Whole server as code (US9, SC-010/011)

1. Give `$LAB` at least one item of every kind: settings, an automod rule, forum tags,
   onboarding, the welcome screen, an emoji, a sticker, a sound, a webhook, a standing invite, a
   future event, and a pinned rules message.
2. Export it:
   ```bash
   GUILDSMITH_FILE_ROOTS=$PWD guildsmith export -g $LAB -o lab.toml   # writes assets/ too
   grep -Ei 'discord(app)?\.com/api/webhooks|token' lab.toml          # no output (SC-011)
   guildsmith plan  -g $LAB lab.toml --detailed-exitcode               # 0
   ```
3. Apply it to the empty server, then plan both:
   ```bash
   guildsmith plan  -g $EMPTY lab.toml                                 # note the hash
   guildsmith apply -g $EMPTY lab.toml --plan-hash <hash> --yes        # webhook URL printed once
   guildsmith plan  -g $EMPTY lab.toml --detailed-exitcode             # 0
   guildsmith plan  -g $LAB   lab.toml --detailed-exitcode             # 0
   ```
4. Edit the rules message text in `lab.toml` and plan. **Expect** an *edit in place* of the same
   message id.
5. Replace `assets/emoji/*.png` with a different image and plan. **Expect** one *replace* op.

## 5. Raw requests (US10, SC-012)

```bash
guildsmith raw GET guilds/{guild}/audit-logs -g $LAB                 # works, output wrapped as untrusted
guildsmith raw PATCH guilds/{guild} -g $LAB --body '{"description":"x"}'          # preview, exit 3
GUILDSMITH_READ_ONLY=1 guildsmith raw PATCH guilds/{guild} -g $LAB --body '{}' --yes   # refused, exit 3
guildsmith raw GET guilds/<id not on allowlist>                      # refused, exit 3
guildsmith raw POST webhooks/<id>/<token>                            # refused: token routes
```
