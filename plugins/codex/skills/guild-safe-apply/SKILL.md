---
name: guild-safe-apply
description: The safe workflow for changing a Discord server's structure with guildsmith - export, edit, validate, plan, show the user, then apply with the plan hash. Use whenever a guild.toml change should reach a live server.
user-invocable: false
---

# Changing a server safely

Never change structure without showing the user a plan first. `spec_apply` only runs a plan whose
hash came from `spec_plan` for the same file and an unchanged server; never invent or reuse a hash.

1. **Export** the current state: `spec_export` with `guild` (add `path` to save a new file; it
   never overwrites). Skip this if the user already has a file.
2. **Edit** the TOML as the user asked (see the guild-spec skill). Keep ids from the export.
3. **Validate** with `spec_validate` (`toml`). Fix every error; mention warnings.
4. **Check the bot** with `bot_preflight` (`guild`, `action = "spec_apply"`, `toml`). Spec apply
   needs Administrator. If `ok` is false, stop and explain the problems (guild-doctor skill).
5. **Plan** with `spec_plan` (`guild`, `toml`). Show the user, in plain words:
   - every op in order with its risk; call out each `destructive` op (deletes and replaces);
   - `unmanaged` items (left alone unless pruning is on), `warnings`, `estimated_requests`;
   - the `plan_hash`.
   If there are no ops, say the server already matches and stop.
6. **Ask** the user to approve this exact plan. Don't apply on an implied yes.
7. **Apply** with `spec_apply` (`guild`, the **same** `toml`, `plan_hash`). Report `applied`, and
   if present `failed` and `skipped`.
8. **Confirm** convergence: run `spec_plan` again; it should have no ops.

## When apply refuses or stops

- **Plan mismatch / server changed**: someone changed the server or the file since the plan.
  Re-plan, show the new plan, and ask again.
- **Preflight refusal**: nothing changed. Explain each problem and how to fix it.
- **Read-only or allowlist refusal**: the operator's policy; tell the user, don't work around it.
- **Stopped partway** (`failed`): earlier ops are done and nothing is retried. Fix the cause, then
  re-plan: the new plan holds only the remaining changes.

CLI equivalent: `guildsmith plan -g <server> guild.toml`, then
`guildsmith apply -g <server> guild.toml --plan-hash <hash>` (add `--yes` only with a reviewed hash).
