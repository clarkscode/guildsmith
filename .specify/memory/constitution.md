<!--
Sync Impact Report
- Version change: 1.0.0 → 1.1.0 (MINOR: materially expanded guidance in principle IV)
- Modified principles: IV. Lean Agent Interface (title unchanged): adds a scoped exception to
  "never raw Discord JSON" for the opt-in `raw` group (`discord_request`, feature 002 US10,
  ADR-005), with the conditions it holds under.
- Added sections: none
- Removed sections: none
- Templates: plan, spec and tasks templates reference the Constitution Check generically; no
  change needed.
- Follow-up: record the decision in docs/adr/005-raw-group.md; resolve T106 in
  specs/002-extended-toolsets/tasks.md.
-->
# guildsmith Constitution

## Core Principles

### I. Safe by Default

guildsmith hands admin power over real communities to people and AI agents, so every write path
MUST be guarded:

- Requests for servers outside the configured allowlist MUST be refused. An empty allowlist MUST
  produce a startup warning.
- Read-only mode MUST block every write.
- Destructive operations MUST return a preview unless explicit confirmation is given.
- Applying a spec MUST require the hash of a plan that was produced and reviewed, and MUST refuse
  if the server's snapshot fingerprint changed since that plan.
- Every write MUST record a reason in Discord's audit log (`guildsmith: <summary>`).
- Permission and role-hierarchy problems MUST be reported together, before any change is made.
- The bot token MUST never appear in logs, errors or tool output. Only bot tokens are supported.
- Message content written by server members MUST be returned as fenced, labeled data, never mixed
  into guildsmith's own instructions.

These checks MUST live in the application layer, so the CLI and the MCP server enforce them
identically. No interface may bypass them.

### II. Pure Core, Thin Adapters

The architecture is hexagonal and dependencies point inward:

- `guildsmith-core` (domain, spec, validation, diff, planner) MUST do no IO and MUST NOT depend on
  Discord, MCP or CLI crates.
- `guildsmith-app` owns use cases, ports (`DiscordPort`, `Clock`) and policy.
- Discord library types MUST NOT leave `guildsmith-discord`. Replacing the Discord library MUST
  only require rewriting that crate.
- Interface crates (MCP, CLI) MUST only translate input and output; they MUST NOT contain business
  rules.

Rationale: the planner is the riskiest logic and must be testable in isolation. Keeping libraries
at the edge keeps the choice of library cheap to reverse.

### III. Plans Converge

Server-as-code is only trustworthy if it is predictable:

- After a successful apply, planning again with the same spec MUST produce an empty plan
  (`diff(spec, apply(spec, s)) == ∅`). This invariant MUST be covered by property tests.
- Plans MUST be deterministic: the same spec and snapshot always yield the same ordered ops and
  the same hash.
- There is no separate state file. Items are matched by `id`, then by unique name in scope;
  ambiguity MUST be an error, never a guess.
- Only items listed in the spec are managed. Unmanaged items MUST NOT be deleted unless `prune` is
  enabled for that kind.
- Apply MUST stop at the first error, report exactly what was applied, and never blindly retry a
  mutation, so the next plan resumes from the remainder.

### IV. Lean Agent Interface

Agent context is a cost paid on every turn, so the MCP surface MUST stay small:

- The default `core` tool group MUST stay at or under 25 tools, and their combined descriptions
  within the token budget (target about 3k). Adding a default tool requires justifying it against
  this budget.
- Extra capabilities go into opt-in tool groups.
- Tool descriptions are one sentence plus parameter docs; longer guidance belongs in plugin skills.
- Results MUST be compact: key fields by default, paging for long lists, never raw Discord JSON.
- **Exception: the opt-in `raw` group.** Its one tool, `discord_request`, MAY return Discord's
  JSON, because it exists for endpoints no dedicated tool covers (ADR-005). The exception holds
  only while all of these do:
  - the group is off by default and the operator can block it (`GUILDSMITH_TOOLSETS_BLOCK`);
  - its description points the agent to the dedicated tools first;
  - every response is wrapped as untrusted content, has tokens and webhook URLs redacted, and is
    cut to 50,000 characters;
  - every other safety rule applies unchanged: the allowlist, read-only mode, a preview until
    confirmation for every method but GET, and an audit-log reason on writes.
  No other tool may return raw Discord JSON.
- Every tool MUST carry a correct `readOnlyHint` or `destructiveHint` annotation.
- stdout is reserved for the MCP protocol; all logging MUST go to stderr.

### V. Tested Without the Network

- Core logic MUST have unit and property tests (diff idempotence, op ordering, permission
  round-trips).
- Services MUST be tested against the in-memory `FakeDiscord` port.
- The Discord adapter MUST be tested with recorded HTTP fixtures.
- MCP tools MUST have protocol tests that call each tool, check results against their schemas and
  measure description size.
- Live tests against a real guild MUST be opt-in (`GUILDSMITH_LIVE_TEST=1`) and MUST NOT be needed
  for a normal test run.
- A bug fix MUST include a test that fails without the fix.

### VI. Performance Budget

guildsmith is started as a child process by agent clients, so startup and size are user-facing:

- Cold start to the first `tools/list` response MUST stay under 50 ms.
- The stripped release binary MUST stay under 10 MB.
- These budgets and the tool-description token budget MUST be checked in CI. A change that
  exceeds them MUST either be fixed or come with an amendment to this constitution.

## Scope & Technology Constraints

- **Out of scope:** creating servers (Discord removed bot server creation), user tokens or
  self-bots, hosting bot logic or slash commands. Features that require these MUST be rejected.
- **v1 connection model:** REST only. Adding the gateway or other persistent connections is a
  scope change that needs a spec.
- **Stack:** Rust (edition 2024), tokio, twilight (REST client and models), rmcp, clap. Changing a
  core dependency requires an ADR in `docs/`.
- **Code rules:** `unsafe_code = "forbid"`; clippy pedantic with documented allows; `thiserror` in
  library crates and `anyhow` only in the binary.
- **Supply chain:** `cargo deny` (licenses, advisories) and `cargo audit` MUST pass.
- **License:** MIT OR Apache-2.0. Dependencies MUST be compatible with it.

## Development Workflow & Quality Gates

- Work follows the Spec Kit flow: specify → (clarify) → plan → tasks → implement. Every
  implementation plan MUST include a Constitution Check against the principles above and justify
  any exception in its Complexity Tracking section.
- CI MUST pass before merge: `cargo fmt --check`, `clippy -D warnings`, all tests, `cargo deny`,
  a minimal-versions check, the performance and token budgets, and a cargo-dist release dry-run.
- Architectural decisions that change PLAN.md §3 MUST be recorded as an ADR in `docs/`.
- Changes to the default tool set, the spec file format or the safety policy MUST be called out
  in the change description and the changelog.

## Governance

This constitution takes precedence over other project practices and guidance. When PLAN.md or a
feature plan conflicts with it, the constitution wins until it is amended.

- **Amendments:** propose the change with its reason, update this file and its Sync Impact Report,
  and update any affected docs in the same change.
- **Versioning:** semantic versioning. MAJOR for removing or redefining a principle, MINOR for
  adding a principle or materially expanding guidance, PATCH for wording and clarifications.
- **Compliance:** every feature plan runs the Constitution Check, and reviews verify that changes
  follow the principles. Unjustified violations block merge.

**Version**: 1.1.0 | **Ratified**: 2026-10-19 | **Last Amended**: 2026-10-20
