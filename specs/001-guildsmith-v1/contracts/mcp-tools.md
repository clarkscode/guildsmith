# Contract: MCP tools

**Server**: `guildsmith mcp` over stdio, MCP 2026-07-28, with support for handshake-based clients
(R3).
**Annotations**: RO = `readOnlyHint: true`. W = writes, additive (`destructiveHint: false`).
D = destructive (`destructiveHint: true`). `idempotentHint` is set where noted (I). Every tool sets
`openWorldHint: true`.

## Shared conventions

- **`guild`** (string): an ID or exact server name. Optional when exactly one server is allowed.
- **Policy refusals** (allowlist, read-only, preflight) come back as tool results with
  `isError: true` and `structuredContent.refusal = { kind, reason, hint }`, not as JSON-RPC
  errors, so the agent can read and act on them.
- **Name inputs** follow data-model §6. If a name is ambiguous, the result has `isError: true`
  and lists `candidates`.
- **Lists** return key fields by default. `fields: [..]` adds more fields, and
  `limit` (default 50, max 200) plus `cursor` page through results. Text output stays under
  50,000 characters.
- **Results** contain a short text summary, plus `structuredContent` that matches the tool's
  `outputSchema`, plus the same JSON as a text block (for clients that ignore
  `structuredContent`).
- **Confirmation**: D tools take `confirm: bool = false`. When it's false, the tool returns
  `{ preview: true, would: [...] }` and changes nothing (FR-018).
- **Audit log**: every write sends `X-Audit-Log-Reason: guildsmith: <summary>`, truncated to 512
  characters. Tools that write accept an optional `reason` (string), which is appended.

## `core` toolset (default, 21 tools)

| Tool | Ann. | Inputs | Output (`structuredContent`) |
|---|---|---|---|
| `guilds_list` | RO | — | `guilds: [{id, name, allowed}]` |
| `guild_overview` | RO | `guild` | `{guild, counts, roles: [{id,name}], tree}`. `tree` is the channel tree with names, kinds and ids. |
| `refresh` | RO, I | `guild?` | `{cleared: [guild ids]}` |
| `spec_export` | RO | `guild`, `path?` | `{toml, path?, warnings}`. `path` writes a new file and refuses to overwrite. |
| `spec_validate` | RO | `toml` | `{valid, problems: [{line, col, message, hint?}]}` (offline) |
| `spec_plan` | RO | `guild`, `toml` | `{plan_hash, risk, ops: [{summary, risk}], unmanaged, warnings, estimated_requests}` |
| `spec_apply` | D | `guild`, `toml`, `plan_hash` | `{applied: [summary], failed?: {summary, error}, skipped: [summary]}`. Refuses if the hash differs (R4). |
| `roles_list` | RO | `guild`, `fields?`, paging | `roles: [{id, name, position, managed}]` |
| `role_upsert` | W, I | `guild`, `role?` (name/id, absent = create), `name?`, `color?`, `hoist?`, `mentionable?`, `permissions?` | `{id, created: bool, changes}` |
| `role_delete` | D | `guild`, `role`, `confirm` | preview or `{deleted}`. Sets `anthropic/requiresUserInteraction`. |
| `channels_tree` | RO | `guild`, `fields?` | `tree: [{id, name, kind, children?}]` |
| `channel_upsert` | W, I | `guild`, `channel?`, `name?`, `kind?`, `parent?`, `topic?`, kind-specific fields | `{id, created, changes}` |
| `channel_delete` | D | `guild`, `channel`, `confirm` | preview or `{deleted}`. Sets `anthropic/requiresUserInteraction`. |
| `channels_reorder` | W, I | `guild`, `order: [channel]`, `parent?` | `{moved: n}` |
| `permissions_set` | W, I | `guild`, `channel`, `target`, `allow?`, `deny?`, `clear?: bool` | `{overwrite}` |
| `permissions_explain` | RO | `guild`, `channel`, `who` (member or role), `permission?` | `{allowed: [..], denied: [..], steps: [{step, source, effect}]}` (R13) |
| `members_find` | RO | `guild`, `query`, paging | `members: [{id, name, roles}]` |
| `member_roles` | W, I | `guild`, `member`, `add?: [role]`, `remove?: [role]` | `{roles}` |
| `message_send` | W | `guild`, `channel`, `content` | `{id, url}` |
| `messages_read` | RO | `guild`, `channel`, `limit?` (default 20, max 100), `before?` | `messages: [{id, author, content, ts}]`. The text block wraps each message as untrusted data (R19). |
| `bot_preflight` | RO | `guild`, `action?` (`spec_apply` with `toml`, or a tool name) | `{ok, problems: [PreflightProblem]}` |

Read-only mode (FR-017) leaves every RO tool working and refuses every W/D tool before it
contacts Discord.

## `moderation` toolset (opt-in: `GUILDSMITH_TOOLSETS=core,moderation`)

| Tool | Ann. | Inputs |
|---|---|---|
| `member_timeout` | W, I | `guild`, `member`, `duration` (≤ 28 days, `0` = clear), `reason?` |
| `member_kick` | D | `guild`, `member`, `reason?`, `confirm` |
| `member_ban` | D | `guild`, `member`, `delete_message_seconds?`, `reason?`, `confirm` |
| `ban_remove` | W | `guild`, `user`, `reason?` |

## Budgets (checked in CI, R16)

- `core`: 25 tools or fewer.
- Tool descriptions: 3,000 tokens or fewer in total.
- Full `tools/list` result: 9,000 tokens or fewer.
- Each description is one sentence. Longer guidance lives in plugin skills.
