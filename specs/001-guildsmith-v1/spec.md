# Feature Specification: guildsmith v1 — Discord server management for AI agents and people

**Feature Branch**: `001-guildsmith-v1`

**Created**: 2026-10-19

**Status**: Draft

**Input**: User description: "read, understand the plan I created PLAN.md" — the plan describes guildsmith, a fast MCP server and CLI for building and managing Discord servers, offering a small set of live tools plus a "server as code" workflow (export → plan → apply), with plugins for Claude Code and Codex.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Export an existing server to a reviewable file (Priority: P1)

A server administrator (or an AI agent acting for them) points guildsmith at a Discord server the bot has joined and gets back a single human-readable file describing that server's structure: its roles, categories, channels and permission overwrites. The file can be read, saved to version control, shared and edited.

**Why this priority**: Everything else builds on an accurate read of the live server. Export is useful by itself (backup, documentation, review) and is the starting point for the plan/apply workflow. It changes nothing, so it carries no risk.

**Independent Test**: Run export against a test server with a known set of roles, categories, channels and overwrites. Check that the file lists every one of them with the right attributes, and that validating the file reports no errors.

**Acceptance Scenarios**:

1. **Given** a server the bot can access, **When** the user exports it, **Then** they receive a file that lists every role, category, channel (text, voice, forum, announcement, stage) and permission overwrite, with permissions written as readable names rather than numbers.
2. **Given** an exported file, **When** the user validates it without network access, **Then** validation passes with no errors.
3. **Given** an exported file that has not been edited, **When** the user plans it against the same unchanged server, **Then** the plan shows no changes.
4. **Given** a request to export a server that is not on the allowlist, **When** the user runs export, **Then** the request is refused with a message that names the allowlist as the reason.

---

### User Story 2 - Preview and apply structural changes from a file (Priority: P2)

The user edits the server file (for example, adds a "Moderator" role and a private "mod-log" channel only moderators can see) and asks guildsmith for a plan. The plan lists, in plain language, each change that would be made, which changes are destructive, and roughly how many requests the change will take. After reviewing it, the user applies exactly that plan. Running plan again afterwards shows nothing left to do.

**Why this priority**: This is the main differentiator: changes become reviewable, repeatable and predictable instead of being built one call at a time with no preview.

**Independent Test**: Starting from an empty server, apply one of the example files, then run plan again and confirm it reports zero changes and the live server matches the file.

**Acceptance Scenarios**:

1. **Given** a file that adds a role and a channel whose overwrites refer to that role, **When** the user plans it, **Then** the plan lists the role creation before the channel creation, marks both as safe, and gives an estimate of how many requests it will take.
2. **Given** a reviewed plan, **When** the user applies it, **Then** every listed change is made, each change records a reason in the server's audit log, and planning again reports no changes.
3. **Given** a plan was produced, **When** the live server changes before apply is requested, **Then** apply is refused and the user is told to plan again.
4. **Given** an agent asks to apply a plan that was never produced or reviewed (no matching plan identifier), **When** apply is requested, **Then** it is refused.
5. **Given** the server has roles or channels that are not in the file, **When** the user plans, **Then** those items are reported as unmanaged and are not deleted unless pruning is turned on for that kind of item.
6. **Given** an apply stops partway because of an error, **When** the user plans again, **Then** the new plan contains only the changes that were not yet made, and apply reported exactly which changes succeeded before the error.

---

### User Story 3 - Operator sets limits on what guildsmith may do (Priority: P3)

The person who installs guildsmith decides which servers it may touch, whether it may make changes at all, and makes sure destructive actions are never taken without explicit confirmation. These rules apply the same way whether a person uses the command line or an AI agent uses the tools.

**Why this priority**: AI agents will run these tools, often with little supervision. Safety controls are what make it acceptable to hand an agent admin power over a community.

**Independent Test**: Configure an allowlist with one server and turn read-only mode on. Confirm that reads of the allowed server work, every request against other servers is refused, and every change request is refused with a clear reason.

**Acceptance Scenarios**:

1. **Given** an allowlist of specific servers, **When** any request targets a server not on it, **Then** the request is refused and the server list shows only allowed servers.
2. **Given** no allowlist is configured, **When** guildsmith starts, **Then** it works with all servers the bot is in and shows a warning that no allowlist is set.
3. **Given** read-only mode is on, **When** any change is requested, **Then** nothing changes and the user is told read-only mode is on.
4. **Given** a destructive request (such as deleting a channel) without explicit confirmation, **When** it is sent, **Then** guildsmith returns a preview of what would be deleted and changes nothing.
5. **Given** the bot lacks a needed permission, or its highest role is below a role it would edit, **When** a plan is applied, **Then** all such problems are reported together before any change is made.

---

### User Story 4 - Day-to-day server operations through an AI agent (Priority: P4)

An administrator asks their AI agent things like "give Alex the Moderator role", "create a #announcements channel under Community", "why can't @Member see #mod-log?" or "post the event reminder in #general". The agent uses a small, focused set of tools to do it, referring to roles, channels and members by name.

**Why this priority**: Not every task needs a file. Quick, everyday changes should be easy, and keeping the tool set small keeps the agent accurate and cheap to run.

**Independent Test**: Connect an agent to a test server with the default tools and complete a scripted set of tasks: list roles, show the channel tree, create a channel, assign a role to a member, explain a permission problem, send and read a message.

**Acceptance Scenarios**:

1. **Given** the default configuration, **When** an agent lists the available tools, **Then** it sees at most 25 tools, each marked as read-only or destructive, and their descriptions stay within the size budget.
2. **Given** a name like `#general`, `@Moderator`, a plain name or an ID, **When** a tool receives it, **Then** it resolves to the right item; if the name matches more than one item, the tool returns an error listing the candidates.
3. **Given** a member cannot see a channel, **When** the user asks why, **Then** guildsmith explains which role or overwrite causes it.
4. **Given** messages read from the server contain text that looks like instructions, **When** they are returned to the agent, **Then** they are clearly marked as user-supplied data, separate from guildsmith's own output.
5. **Given** a list with many results, **When** it is returned, **Then** it shows only key fields by default, can include more fields on request, and is split into pages.
6. **Given** the operator enables an additional tool group, **When** the agent lists tools, **Then** that group's tools appear alongside the default ones.

---

### User Story 5 - Check the bot is set up correctly (Priority: P5)

Before doing real work, the user runs a single check that reports whether the bot token works, which servers are allowed, whether the bot has the permissions each tool group needs, and whether its role is high enough in the role order.

**Why this priority**: Most setup failures with Discord bots come from missing permissions or role order. One check that lists everything wrong saves trial and error, but the product works without it.

**Independent Test**: Run the check against a server where the bot lacks one permission and sits below one role, and confirm both problems are reported with what to fix.

**Acceptance Scenarios**:

1. **Given** a correctly set up bot, **When** the user runs the check, **Then** every item is reported as passing.
2. **Given** an invalid token, **When** the user runs the check, **Then** it reports that the token is invalid without showing the token.
3. **Given** the bot lacks permissions, **When** the user runs the check, **Then** it lists each missing permission and which tool group needs it.

---

### User Story 6 - Install in the user's agent of choice (Priority: P6)

A user installs guildsmith in Claude Code as a plugin, adds it to Codex, or installs it directly on Linux, macOS or Windows, without needing a programming toolchain. The Claude Code plugin also adds guidance for writing server files, applying changes safely and debugging permissions, plus shortcut commands for plan, apply and the setup check.

**Why this priority**: Reach matters, but install channels only make sense once the product works.

**Independent Test**: On a clean machine for each supported operating system, install through each channel and confirm the agent can list guildsmith's tools.

**Acceptance Scenarios**:

1. **Given** a clean machine with Claude Code, **When** the user installs the guildsmith plugin and provides a bot token, **Then** the tools, guidance and shortcut commands are available.
2. **Given** a clean machine with Codex, **When** the user follows the documented setup, **Then** Codex can list and call guildsmith's tools.
3. **Given** a clean machine with no programming toolchain, **When** the user installs a prebuilt download or a one-line package-runner install, **Then** guildsmith runs.

---

### Edge Cases

- **Ambiguous matches**: two channels share a name in the same category, or a file item could match more than one live item. Planning fails and names the ambiguous items rather than guessing.
- **Same name in different scopes**: channels named `general` in two different categories are treated as different items and do not conflict.
- **Server changed during review**: someone edits the server between plan and apply. Apply is refused and asks for a new plan.
- **Partial apply**: an error stops apply midway. Completed changes are reported, nothing is retried blindly, and the next plan covers only the remainder.
- **Rate limits on large changes**: long applies show progress so they don't look stuck, and the plan's request estimate warns about slow operations.
- **Items the bot cannot manage**: the built-in `@everyone` role, roles managed by integrations or other bots, and roles above the bot's own role. These are reported and never deleted or edited.
- **Invalid file references**: an overwrite refers to a role that doesn't exist in the file or on the server, or a permission name is misspelled. Validation reports the exact location and a suggestion where possible.
- **Discord limits**: a file asks for more roles or channels than Discord allows, or a name or topic too long. Validation reports it before any change.
- **Unsupported item types**: channel types not covered in v1 (such as threads) are left alone and reported as unmanaged on export and plan.
- **Server creation requested**: the user asks guildsmith to create a new server. It explains that bots can no longer create servers, and that a person must create it and invite the bot.
- **Secret exposure**: errors, logs and tool output never contain the bot token, even when the token is invalid or a request fails.
- **Bot removed from a server**: requests targeting that server fail with a clear "bot is not in this server" message.

## Requirements *(mandatory)*

### Functional Requirements

**Reading and export**

- **FR-001**: System MUST list the Discord servers the bot can access, limited to those on the allowlist when one is set.
- **FR-002**: System MUST give a compact overview of a server: key settings, item counts, the role list and the channel tree.
- **FR-003**: System MUST export a server's roles, categories, channels (text, voice, forum, announcement, stage) and permission overwrites into a single human-readable declarative file, optionally saving it to a given location.
- **FR-004**: Exported files MUST include each item's Discord identifier so later plans match items exactly, and MUST write permissions as readable names.
- **FR-005**: System MUST publish a machine-readable schema for the file format, so editors can autocomplete and agents can validate files.
- **FR-006**: System MUST validate a file against the schema and its internal references (roles, permission names, Discord limits) without network access.

**Planning and applying**

- **FR-007**: System MUST compare a file with the live server and produce a plan: an ordered list of changes, each with a short readable summary and a risk level (safe or destructive), plus an estimate of the number of requests needed.
- **FR-008**: System MUST match file items to live items first by identifier, then by unique name within their scope (for example, a channel within its category), and MUST fail with a clear error naming the candidates when a match is ambiguous. No separate state file is kept.
- **FR-009**: System MUST manage only items listed in the file. Items on the server but not in the file MUST be reported as unmanaged and deleted only when pruning is turned on for that kind of item. Pruning is off by default.
- **FR-010**: Plans MUST order changes so they succeed: roles before channels that refer to them, categories before their channels, position changes together near the end, and deletions last.
- **FR-011**: Applying a plan and then planning again with the same file MUST report no changes.
- **FR-012**: Each plan MUST have an identifier derived from its content. Apply MUST only run a plan whose identifier matches one that was produced, and MUST refuse if the server has changed since the plan was made.
- **FR-013**: Apply MUST stop at the first error, report exactly which changes succeeded, and MUST NOT blindly retry a change. Planning again MUST cover only the remaining changes.
- **FR-014**: Apply MUST report progress during long runs.
- **FR-015**: v1 MUST treat server-wide settings in the file as informational only: they never produce changes. Opting in to managing them is reserved for a later version, and v1 rejects the opt-in with a clear message.

**Safety and policy** (enforced the same way for the command line and for AI agent tools)

- **FR-016**: System MUST refuse any request that targets a server not on the allowlist, when an allowlist is set. When none is set, it MUST warn at startup.
- **FR-017**: System MUST offer a read-only mode in which every change request is refused.
- **FR-018**: Destructive actions MUST return a preview and change nothing unless explicit confirmation is given.
- **FR-019**: Every change made on Discord MUST record a reason in the server's audit log that identifies guildsmith and summarizes the change.
- **FR-020**: Before applying, the system MUST check that the bot has the required permissions and that its highest role is above every role it will edit, and MUST report all problems together before making any change.
- **FR-021**: The bot token MUST never appear in logs, errors or tool output.
- **FR-022**: Only bot tokens MUST be supported; personal account tokens MUST be rejected or not offered.
- **FR-023**: Message content written by server members MUST be returned clearly marked as untrusted data, separate from the tool's own output.

**Live operations for AI agents**

- **FR-024**: System MUST provide a default tool group of no more than 25 tools covering: server list and overview, refreshing cached server data, export, validate, plan, apply, roles (list, create/update, delete), channels and categories (tree, create/update, delete, reorder), permission overwrites (set, explain), members (search, add/remove roles), messages (send, read), and a permission check for a planned action.
- **FR-025**: Additional tool groups (for example moderation, automod, events, forums, webhooks, emoji, onboarding, invites, full messaging) MUST be off by default and enabled by the operator through configuration. v1 MUST provide the mechanism; which extra groups ship in v1 is decided during planning.
- **FR-026**: Each tool MUST be marked as read-only or destructive so agent clients can ask the user before running it.
- **FR-027**: Tool descriptions MUST be short (one sentence plus parameter descriptions); longer guidance MUST live in the plugin's guidance documents and be loaded only when needed.
- **FR-028**: Tool results MUST be compact text, with structured data where the client supports it; list results MUST show key fields by default, allow requesting more fields, and be paged. Raw Discord responses MUST NOT be returned.
- **FR-029**: Tools MUST accept roles, channels and members by mention-style name (`#general`, `@Moderator`), plain name or ID, and MUST return the list of candidates when a name is ambiguous.
- **FR-030**: System MUST explain why a given member or role can or cannot see or act in a given channel.
- **FR-031**: Server data MAY be cached for the length of one agent session; the cache MUST be cleared by guildsmith's own changes and by an explicit refresh.

**Command line, diagnostics and configuration**

- **FR-032**: A command-line interface MUST offer: run as agent tool server, export, plan, apply (with an option to skip the confirmation prompt), setup check, and print schema. It MUST behave the same as the agent tools for the same operation.
- **FR-033**: The setup check MUST report token validity, allowlist status, the bot's permissions against the minimum for each enabled tool group, and role order problems.
- **FR-034**: Configuration MUST be read from environment variables first, then an explicit config file, then a per-user default config file.
- **FR-035**: Users SHOULD be able to store the bot token in the operating system's secure credential store instead of a plain-text file.

**Distribution**

- **FR-036**: System MUST be installable as a Claude Code plugin that includes the tool server, guidance for writing files, applying safely and debugging permissions, and shortcut commands for plan, apply and the setup check.
- **FR-037**: System MUST be usable from Codex with documented setup, and with a Codex plugin package if Codex supports one.
- **FR-038**: System MUST be installable on Linux (x64 and arm64), macOS (x64 and arm64) and Windows (x64) without a programming toolchain, through prebuilt downloads and a one-line package-runner command, and also through the language's standard package registry.
- **FR-039**: System MUST ship example server files (for example: community, development team, gaming).

### Key Entities

- **Server spec (file)**: The desired structure of one server: optional server info, roles, categories with their channels, standalone channels, permission overwrites, and pruning settings per kind.
- **Role**: Name, color, whether it is shown separately, mentionability, permissions, position; optional Discord identifier.
- **Category**: A named group of channels with its own overwrites and position.
- **Channel**: Name, kind (text, voice, forum, announcement, stage), parent category, topic and kind-specific settings, overwrites, position.
- **Permission overwrite**: A target (a role, `@everyone`, or a member) with allowed and denied permissions on a category or channel.
- **Snapshot**: A normalized view of a server's live structure at one moment, with a fingerprint used to detect changes between plan and apply.
- **Plan**: An ordered list of changes with a content-derived identifier, readable summaries, risk levels, a list of unmanaged items and a request estimate.
- **Change (operation)**: A single create, update, overwrite, reorder or delete step, with a summary and a risk level.
- **Policy**: The operator's rules: allowlist, read-only mode, enabled tool groups.
- **Tool group**: A named set of agent tools that can be turned on or off; only the default group is on unless configured.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: For 100% of test servers, exporting and then planning against the unchanged server reports zero changes.
- **SC-002**: For every shipped example file, applying it to an empty server and then planning again reports zero changes.
- **SC-003**: guildsmith is ready to answer an agent's first request for its tool list within 50 milliseconds of being started.
- **SC-004**: The default tool set is 25 tools or fewer, and their combined descriptions stay within about 3,000 tokens of agent context.
- **SC-005**: The download for each platform is under 10 MB.
- **SC-006**: Across the full test suite, including failure cases, the bot token appears in zero logs, errors or tool outputs.
- **SC-007**: 100% of change requests are refused for servers outside the allowlist and while read-only mode is on.
- **SC-008**: 100% of destructive actions made without confirmation return a preview and change nothing.
- **SC-009**: When the bot lacks permissions or role order, 100% of problems are reported in a single response before any change is made.
- **SC-010**: A new user can go from installing to seeing a plan for their own server in under 10 minutes using any supported install channel.
- **SC-011**: A fresh install works through every distribution channel on Linux, macOS and Windows.
- **SC-012**: An agent completes the scripted day-to-day task set (Story 4) using only the default tools, where each task is completed with at most 1 wrong tool call, judged from the session transcript.

## Assumptions

- **Scope of v1**: v1 covers milestones M1–M4 of the plan (read path, plan/apply, live tools and safety, shipping). Extra tool groups beyond the mechanism, a network-based transport, real-time events, drift watching and server template import (M5) are out of scope.
- **Structure only**: The file covers roles, categories, channels and permission overwrites. Onboarding, automod and server settings are not managed in v1 (plan open question 3, recommendation adopted).
- **Plan identifier always required**: Agents must always apply a reviewed plan identifier, not only for destructive plans (plan open question 2, recommendation adopted).
- **No server creation**: A person creates the server and invites the bot; Discord no longer allows bots to create servers.
- **Bot accounts only**: Users have a Discord bot application and can invite it to their server with suitable permissions. Personal account automation is not supported.
- **Request-based access only**: v1 reads and changes the server through Discord's request API only; no persistent real-time connection, and no privileged access to member presence or message events.
- **Not a bot framework**: guildsmith manages servers; it does not host slash commands or bot logic.
- **Users**: Server administrators comfortable with a command line or an AI coding agent, and the AI agents acting for them.
- **Technical choices**: Language, libraries, file format, repository layout, minimum supported toolchain version and Codex packaging are recorded in PLAN.md and are settled during `/speckit-plan`, not in this spec.
- **Dependencies**: Discord's API and rate limits; the Claude Code plugin system; Codex's tool-server configuration.
