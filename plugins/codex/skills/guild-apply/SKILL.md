---
name: guild-apply
description: Apply a guild.toml plan the user has already reviewed to a Discord server with guildsmith, using its plan hash.
user-invocable: true
disable-model-invocation: true
argument-hint: "[file] [server] [plan-hash]"
---

# /guild-apply

Apply a plan the user has reviewed. Follow the guild-safe-apply skill; this command is its last
steps.

1. Identify the file, the server and the plan hash: from the arguments, or from a `spec_plan` shown
   to the user earlier in this conversation. If there is no reviewed plan, run `/guild-plan` first,
   show it, and stop until the user approves. Never invent a `plan_hash`.
2. Make sure the file text is exactly what was planned. If it changed, re-plan and ask again.
3. Run `bot_preflight` with `guild`, `action = "spec_apply"` and `toml`. If `ok` is false, explain
   the problems (guild-doctor skill) and stop.
4. Run `spec_apply` with `guild`, the same `toml` and `plan_hash`.
5. Report what happened:
   - **Applied**: list `applied`, then run `spec_plan` again and confirm it has no ops.
   - **Stopped partway**: list `applied`, the `failed` op with its error, and `skipped`. Nothing was
     retried. Suggest a fix, then re-plan; the new plan holds only what's left.
   - **Refused**: nothing changed. A plan mismatch means the server or file changed since the plan:
     re-plan and ask the user again. Read-only, allowlist and preflight refusals come from the
     operator's policy or the bot's setup; explain them.
