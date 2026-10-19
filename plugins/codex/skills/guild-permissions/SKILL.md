---
name: guild-permissions
description: Debug why a Discord member or role can or can't see, post, or speak in a channel using guildsmith's permissions_explain, and fix it with an overwrite. Use for "can't see", "can't post" or "why can X do Y" questions.
user-invocable: false
---

# Debugging channel permissions

1. **Explain**: call `permissions_explain` with `guild`, `channel` (`#name`, `Category/name`, name
   or id), `who` (a member like `@alex` or a role like `@Moderator`) and, when the question is about
   one thing, `permission` (e.g. `view_channel`, `send_messages`, `connect`).
2. **Read the steps** in order. Discord computes permissions like this, and each step names its
   source:
   1. `owner`: the server owner has everything.
   2. `base`: `@everyone` plus every role the member has.
   3. `administrator`: Administrator in the base grants everything; overwrites are skipped.
   4. `everyone_overwrite`: the channel's `@everyone` overwrite, deny then allow.
   5. `role_overwrites`: all of the member's role overwrites combined, deny then allow.
   6. `member_overwrite`: the member's own overwrite.
   7. `implicit`: no `view_channel` removes everything; no `send_messages` removes message
      permissions; no `connect` removes voice permissions; a timed-out member keeps only
      `view_channel` and `read_message_history`.
   The last step that changed the permission decided it. Tell the user which role or overwrite
   that was.
3. **Fix** with the smallest change the user agrees to:
   - a channel overwrite: `permissions_set` (`guild`, `channel`, `target`, `allow`, `deny`, or
     `clear = true` to remove the target's overwrite);
   - a role's base permissions: `role_upsert` with `role` and `permissions`;
   - a member's roles: `member_roles` with `add` / `remove`.
   If the server is managed from a guild.toml, change the file instead and use guild-safe-apply.
4. **Verify**: run `permissions_explain` again.

The bot can only allow or deny permissions it has itself, and only an administrator can put
`manage_roles` in an overwrite. If a fix is refused for that reason, see the guild-doctor skill.
