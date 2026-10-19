# Data Model: guildsmith v1

**Plan**: [plan.md](plan.md) · **Research**: [research.md](research.md)

All types below live in `guildsmith-core` and do no I/O, except where noted. IDs are Discord
snowflakes, stored as `u64` and serialized as strings. The user-facing file format is in
[contracts/spec-format.md](contracts/spec-format.md).

## 1. Spec: the desired state, parsed from the spec file

### GuildSpec
| Field | Type | Rules |
|---|---|---|
| `guild` | `GuildInfo` | Informational only (R21). |
| `roles` | `Vec<RoleSpec>` | Unique names (case-sensitive). At most 250 including existing unmanaged roles. |
| `categories` | `Vec<CategorySpec>` | Unique names. |
| `channels` | `Vec<ChannelSpec>` | Top-level channels with no category. |
| `prune` | `PruneSpec` | Defaults to `false` for every kind. |

Cross-item rules (checked offline by `spec_validate`):
- At most 500 channels including categories.
- At most 50 channels per category.
- Every overwrite `target` must resolve to a spec role, `@everyone`, an existing-role id, or a
  member id (`member:<id>`).
- Every permission name must be known.
- `manage_settings = true` is rejected (R21).

### GuildInfo
`name: Option<String>`, `id: Option<Id>`, `manage_settings: bool = false`.

### RoleSpec
| Field | Type | Rules |
|---|---|---|
| `id` | `Option<Id>` | Written by export. |
| `name` | `String` | 1–100 characters. `@everyone` is reserved for the default role. |
| `color` | `Option<Color>` | `#RRGGBB`. The primary color only (R2). |
| `hoist` | `Option<bool>` | |
| `mentionable` | `Option<bool>` | |
| `permissions` | `Option<PermissionSet>` | |
| `position` | `Option<u16>` | Optional. Without it, order follows the order of roles in the file (first = highest). |
| `managed` | `bool = false` | Must match the live role. If `true`, only `permissions` may differ (R12). |

`None` means "not managed by this spec": the planner leaves that live attribute unchanged.

### CategorySpec
`id`, `name` (1–100), `position: Option<u16>`, `overwrites: Vec<OverwriteSpec>`,
`channels: Vec<ChannelSpec>`.

### ChannelSpec
| Field | Type | Rules |
|---|---|---|
| `id` | `Option<Id>` | |
| `name` | `String` | 1–100. Text-like kinds are lowercased (warning if the spec isn't). |
| `kind` | `ChannelKind` | `text` \| `announcement` \| `voice` \| `stage` \| `forum` \| `media`. |
| `topic` | `Option<String>` | 0–1024 characters, or 0–4096 for forum/media. Not allowed on voice. |
| `nsfw` | `Option<bool>` | |
| `slowmode` | `Option<Duration>` | 0–21600 s. Not allowed on announcement. |
| `bitrate` / `user_limit` / `rtc_region` | voice/stage only | bitrate ≥ 8000; user_limit 0–99 for voice, ≤ 10000 for stage. |
| `tags` | `Vec<ForumTag>` | forum/media only. At most 20; names ≤ 20 characters. |
| `default_reaction` | `Option<Emoji>` | forum/media only. |
| `sync_permissions` | `bool = false` | If `true`, the channel's effective overwrites are the category's, and `overwrites` must be empty. |
| `overwrites` | `Vec<OverwriteSpec>` | |
| `replace` | `bool = false` | Allows a kind change by delete + create (R10). |

### OverwriteSpec
`target: Target` (`@everyone` \| `@RoleName` \| `role:<id>` \| `member:<id>`),
`allow: PermissionSet`, `deny: PermissionSet`. The same permission may not appear in both
`allow` and `deny`. Each target may appear only once per channel.

### PermissionSet / Permission
- A `Permission` enum covers bits 0–52 (R2 table), with `snake_case` names
  (`view_channel` ↔ bit 10).
- `PermissionSet` is a bit set. It serializes as a sorted list of names, so output is stable.
- Unknown bits from Discord are kept as `unknown_<bit>` so a round trip never loses data.

### PruneSpec
`roles: bool`, `categories: bool`, `channels: bool`, all `false` by default.

## 2. Snapshot: the live state, built by the adapter

### Snapshot
| Field | Notes |
|---|---|
| `guild` | id, name, owner_id, features (e.g. `NEWS`, `ENHANCED_ROLE_COLORS`), premium tier |
| `roles` | `Vec<LiveRole>`: the spec role fields plus `managed`, `tags`, `secondary_color`/`tertiary_color` (read-only) |
| `channels` | `Vec<LiveChannel>`: all kinds, including unsupported ones (`Other(u8)`), with `parent_id` and `position` |
| `bot` | the bot's user id, role ids, highest role position, guild-level permissions |
| `completeness` | `Complete` (the bot is an administrator) \| `MaybeIncomplete` (R6) |
| `fingerprint` | blake3 of the canonical snapshot (R4) |

The snapshot is normalized: lists are sorted by id, and permission sets are canonical. Two
fetches of an unchanged server produce the same fingerprint.

## 3. Plan

### Plan
| Field | Notes |
|---|---|
| `hash` | `gsp_<32 hex>` (R4) |
| `ops` | `Vec<Op>`, in execution order (R8) |
| `unmanaged` | live items not in the spec and not pruned |
| `warnings` | stale ids, lowercased names, role-creation volume, incomplete snapshot |
| `estimated_requests` | `u32` (R9) |
| `risk` | `safe` if every op is safe, otherwise `destructive` |
| `snapshot_fingerprint` | used to build the hash |

### Op
Each `Op` has `summary: String` (shown to people, e.g. `create role @Moderator`),
`risk: Risk` (`Safe` | `Destructive`) and `target: ItemRef`.

| Variant | Risk | Notes |
|---|---|---|
| `CreateRole { spec }` | safe | |
| `UpdateRole { id, changes }` | safe | `changes` is a field-level diff, shown as `old → new` |
| `ReorderRoles { order }` | safe | one request |
| `CreateChannel { spec, parent }` | safe | covers categories too (`kind = category`) |
| `UpdateChannel { id, changes, overwrites }` | safe | one PATCH: fields + full overwrite list (R7) |
| `ReorderChannels { positions }` | safe | one request, positions only (R8) |
| `DeleteChannel { id }` | destructive | only with `prune.channels` / `prune.categories`, or as part of a replace |
| `DeleteRole { id }` | destructive | only with `prune.roles`. Never for `@everyone`, managed roles, or roles at/above the bot |

A **replace** (R10) is a `DeleteChannel` followed by a `CreateChannel` that share a
`replace_group`, so a reader sees them as one change.

### Ordering invariants (property-tested)
1. Every `CreateRole` comes before any op whose overwrites refer to that role.
2. A category's create comes before its children's creates.
3. At most one `ReorderRoles` and one `ReorderChannels`, both after every create and update.
4. Every delete comes after every non-delete op.
5. `diff(spec, apply_fake(spec, snapshot)).ops.is_empty()` (constitution III).

## 4. Apply

### ApplyReport
`applied: Vec<OpResult>` (in order, with the created ids), `failed: Option<(Op, Error)>`,
`skipped: Vec<Op>` (everything after the failure).

### Plan lifecycle
```
spec + snapshot ──plan──▶ Planned(hash)
Planned ──apply(hash)──▶ fresh snapshot → re-plan
   ├─ hash differs ──▶ Refused(ServerChangedOrPlanMismatch)   [nothing changed]
   ├─ preflight fails ──▶ Refused(Preflight{problems})          [nothing changed]
   └─ ok ──▶ Applying ──▶ Applied        (re-plan ⇒ empty)
                       └─▶ Partial(report)  (re-plan ⇒ the remaining ops only)
```

## 5. Policy and configuration (`guildsmith-app`)

### Policy
`allowlist: Option<BTreeSet<Id>>` (`None` means all servers, with a startup warning),
`read_only: bool`, `toolsets: BTreeSet<Toolset>`.

### Preflight problems (`PreflightProblem`)
`MissingPermission { perm, needed_by }`, `RoleTooHigh { role, bot_top }`,
`NotAdministrator { reason }` (R6/R7), `CannotGrant { perm, channel }`,
`ManagedRoleEdit { role, field }`. All problems are collected before anything is reported
(FR-020).

### Config
`token: Secret` (redacting newtype, R14), `allowlist`, `read_only`, `toolsets`, `log`.
Sources are applied in this order: env vars, then `--config`, then the default config file, with
the token also looked up in the keyring.

## 6. Name resolution (`guildsmith-app`)

`resolve_role("@Moderator" | "Moderator" | "<id>")`, `resolve_channel("#general" | "general" |
"Community/general" | "<id>")`, `resolve_member("@name" | "name" | "<id>")`.

Each returns `Resolved(id)` or `Ambiguous(Vec<Candidate>)` (with name, id and parent), or
`NotFound { suggestions }`. Suggestions are the closest names by edit distance, up to 3.
