---
name: guild-plan
description: Plan a guild.toml file against a Discord server with guildsmith and summarize what would change, without changing anything.
user-invocable: true
argument-hint: "[file] [server]"
---

# /guild-plan

Plan a server file and explain the result. This never changes the server.

1. Find the file: the path in the arguments, otherwise `guild.toml` in the working directory, otherwise
   ask. Read its text.
2. Find the server: the id or name in the arguments, otherwise `[guild] id` in the file; if exactly
   one server is allowed, `guild` may be omitted. Use `guilds_list` if unsure.
3. Run `spec_validate` with the text. If it reports errors, list them with line and column and stop.
4. Run `spec_plan` with `guild` and `toml`.
5. Summarize:
   - **No ops**: "The server already matches the file."
   - Otherwise list each op in order, grouped as roles, categories, channels, reorders and deletes,
     and mark every `destructive` op clearly.
   - `unmanaged` items (not deleted unless `[prune]` is on), `warnings`, `estimated_requests`.
   - The `plan_hash`, and that applying needs `/guild-apply` with this hash. Any change to the
     file or the server makes the hash invalid.

Don't call `spec_apply` from this command.
