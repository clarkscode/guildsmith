---
name: guild-doctor
description: Check that the guildsmith Discord bot is set up correctly for a server (token, allowlist, permissions, role order) and explain how to fix each problem.
user-invocable: true
argument-hint: "[server]"
---

# /guild-doctor

1. Run `guilds_list`. If it's empty, the bot is in no allowed server: the user must create a server
   (bots can't) and invite the bot, and add the server id to `GUILDSMITH_GUILDS` if an allowlist is
   set. A refusal mentioning the token means `DISCORD_TOKEN` is missing, invalid or not a bot token;
   never ask the user to paste the token into the chat.
2. Pick the server from the arguments, or the only allowed one; otherwise ask.
3. Run `bot_preflight` with `guild`. If the user is about to apply a file, also pass
   `action = "spec_apply"` and its `toml`.
4. If `ok` is true, say everything passes. Otherwise explain every problem and its fix:
   - `MissingPermission { perm, needed_by }`: give the bot's role `perm` (Server Settings → Roles),
     needed by the named tools.
   - `RoleTooHigh { role, bot_top }`: the bot can only manage roles below its own highest role. Drag
     the bot's role above `role` in Server Settings → Roles.
   - `NotAdministrator`: applying a guild.toml needs Administrator. Without it Discord hides
     private channels from the bot, and the bot can't grant permissions it lacks.
   - `CannotGrant { perm, channel }`: an overwrite would grant `perm`, which the bot doesn't have.
     Give the bot that permission, or Administrator.
   - `ManagedRoleEdit { role, field }`: bot, integration and booster roles are managed by Discord;
     only their `permissions` can change.
5. After the user fixes things, run `bot_preflight` again.

CLI equivalent: `guildsmith doctor -g <server>` (exit 0 when every check passes, 3 otherwise).
