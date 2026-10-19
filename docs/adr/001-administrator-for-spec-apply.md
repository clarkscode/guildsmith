# ADR-001: Administrator is required for spec apply

- Status: accepted
- Date: 2026-10-19
- Context: research R6, R7 in `specs/001-guildsmith-v1/research.md`

## Context

- From 2026-11-16, Discord leaves channels the bot cannot view out of `GET /guilds/{id}/channels`.
  Without care, a private channel would vanish from exports, and apply would try to create it
  again as a duplicate.
- A bot can only allow or deny permissions it has itself, and only an administrator can put
  `MANAGE_ROLES` in a channel overwrite.

## Decision

- `spec_apply` / `guildsmith apply` require the bot to have `ADMINISTRATOR`. `doctor` and
  `bot_preflight` report anything less as a blocking problem for apply.
- Live tools keep working with the narrower permissions listed per toolset.
- The planner treats a spec `id` that is missing from the snapshot as a hard error when the bot
  is not an administrator ("missing or hidden from the bot"), never as a create.

## Consequences

- Cautious operators have to grant a powerful role. `doctor` explains why, and the allowlist plus
  the reviewed-plan check limit what that role is used for.
- Exports by a non-administrator bot carry a header warning that hidden channels may be missing.
