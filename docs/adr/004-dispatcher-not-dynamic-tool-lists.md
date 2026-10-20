# ADR-004: Dispatcher tools instead of dynamic tool lists

- Status: accepted
- Date: 2026-10-20
- Context: spec 002 FR-003a–c and US8; research E1

## Decision

- `tools/list` stays fixed for the life of the process. The groups the operator registers at
  startup (`GUILDSMITH_TOOLSETS`, including `all`) are listed as normal tools.
- Two dispatcher tools join the default set, which grows from 21 to 23 tools:
  - `toolsets` (`action = list | describe | enable`) lists every group with its tool count,
    approximate description tokens and required permissions, returns a group's tool
    definitions, and unlocks a group for this process.
  - `tool_call` (`tool`, `arguments`) runs a tool in an unlocked group through the same
    handler as a direct call and returns its result unchanged.
- Groups the operator blocks (`GUILDSMITH_TOOLSETS_BLOCK`) can't be enabled, and the block wins
  over `GUILDSMITH_TOOLSETS`. Every enablement is logged to stderr at `info` with the time, the
  group and who enabled it. An enablement lasts until the process exits.
- The agent can change only the set of unlocked groups. The allowlist, read-only mode and the
  block list are read-only to every tool.

## Why not change the tool list at runtime

1. MCP 2026-07-28 says the tool list "MUST NOT vary … as a side effect of other requests on the
   connection". An `enable` call that grew `tools/list` would be exactly that.
2. Codex ignores `notifications/tools/list_changed` by default (its `mcp_tool_list_changed`
   feature flag is off), so tools added at runtime would never appear there.
3. GitHub removed its similar "dynamic toolsets" feature from github-mcp-server on 2026-05-20;
   it had no fallback for clients that ignore list changes.

A dispatcher works in every client, so the agent can use a newly enabled group even when its
client can't refresh its tool list (FR-003a).

## Consequences

- The default set is 23 tools, still within the constitution's 25-tool and ~3k-token budget.
- A client's "always allow" on `tool_call` covers every tool behind it. The real controls stay
  inside guildsmith: previews until `confirm`, the allowlist, read-only mode and audit reasons
  apply to dispatched calls exactly as to direct ones. The skills docs explain the trade-off.
- `toolsets(enable)` changes guildsmith's own state, not Discord's, so `toolsets` is annotated
  read-only. `tool_call` carries a destructive hint because it can reach destructive tools.
