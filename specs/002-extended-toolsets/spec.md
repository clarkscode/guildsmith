# Feature Specification: Extended toolsets — full Discord server coverage

**Feature Branch**: `002-extended-toolsets`

**Created**: 2026-10-19

**Status**: Draft

**Input**: User description: "I want to add more tools to guildsmith" — close the gaps against
existing Discord MCP servers: moderation (ban, timeout, prune); forums, threads, tags; webhooks,
scheduled events, emoji/stickers; voice/stage, invites, auto-moderation, polls, server templates.
"…so that I can utilize all tools and do anything."

**Builds on**: [001-guildsmith-v1](../001-guildsmith-v1/spec.md). v1 provides the default `core`
tools, a small `moderation` group, the opt-in tool group mechanism, and the safety policy
(allowlist, read-only mode, confirmations, audit reasons, preflight).

## Context

| Area | Other Discord MCP servers | guildsmith v1 | This feature |
|---|---|---|---|
| Roles, channels, categories, permissions | ✅ | ✅ plus plan/apply | unchanged |
| Messages, member roles | ✅ | ✅ basic | full messaging |
| Moderation (ban, timeout, prune) | ✅ | partial (timeout, kick, ban, unban) | complete |
| Forums, threads, tags | ✅ | ❌ | ✅ |
| Webhooks, scheduled events, emoji/stickers | ✅ | ❌ | ✅ |
| Voice/stage, invites, auto-moderation, polls, server templates | ✅ in some | ❌ | ✅ |

The goal is that an administrator, through their AI agent or the command line, can do anything
in a server that Discord allows a bot to do, in three ways:

1. **Dedicated tools** for every area, in opt-in tool groups.
2. **Server as code** for everything that is lasting configuration, not just roles and channels
   (Q1 → C).
3. **A raw request tool** for anything the dedicated tools don't cover (Q2 → C).

The default experience stays small and cheap: the default tool set does not change, and the
agent can turn on more groups when it needs them (Q3 → C).

### Clarifications

#### Session 2026-10-19

- Q: Should the new areas also be manageable through the spec file? → A: **C**. Everything that
  can be described as lasting configuration, including webhooks and invites.
- Q: Should there be a general-purpose tool for any Discord endpoint? → A: **C**. A full raw tool
  (read and write) in its own opt-in group. Every write needs confirmation and an audit reason.
- Q: Can the agent turn on tool groups by itself? → A: **C**. The agent can enable any group
  during a session.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Full moderation (Priority: P1)

A moderator asks their agent to deal with a raid or a bad actor: "ban these 30 accounts that
joined in the last hour and delete their messages from today", "time out @spammer for a day",
"remove members who haven't been active for 30 days and have no roles", "show me the ban list",
"delete the last 50 messages in #general from @spammer".

**Why this priority**: Moderation is the most frequent urgent admin task, and the area where an
agent saves the most time. It is also the riskiest, so it sets the safety pattern that the other
groups follow.

**Independent Test**: With only the moderation group enabled on a test server, perform each
moderation action against test accounts. Confirm that every destructive action first shows a
preview with the exact count, and that nothing happens without confirmation.

**Acceptance Scenarios**:

1. **Given** a list of members to ban, **When** the agent asks without confirmation, **Then** it gets a preview naming each member (or the count, for long lists) and nothing happens.
2. **Given** confirmation, **When** a bulk ban of up to 200 members runs, **Then** each one is banned with the audit reason, and the result lists any that failed and why.
3. **Given** a member prune request (inactive for N days, optionally limited to certain roles), **When** previewed, **Then** it reports how many members would be removed before anything happens.
4. **Given** a request to delete recent messages from one author in a channel, **When** confirmed, **Then** matching messages are deleted, and messages too old for bulk deletion are reported rather than silently skipped.
5. **Given** the ban list is requested, **When** it has many entries, **Then** it is returned in pages with the ban reasons.

---

### User Story 2 - Full messaging, threads and forums (Priority: P2)

A community manager asks the agent to "post the weekly update as a thread in #announcements",
"create a forum post in #help tagged 'question'", "add a 'solved' tag to the forum and apply it to
these posts", "pin this message", "edit yesterday's announcement", "close and lock threads with
no activity for a week", "react with ✅ to these messages", "run a poll: which day for game
night?" and "show the poll results".

**Why this priority**: Day-to-day community management is mostly about content. v1 covers only
sending and reading messages.

**Independent Test**: With the messaging and forum groups enabled, carry out each request on a
test server and confirm the result in Discord.

**Acceptance Scenarios**:

1. **Given** a channel and a message, **When** the agent edits, deletes, pins, unpins or reacts, **Then** the change appears in Discord. Deleting asks for confirmation first.
2. **Given** a forum channel, **When** the agent creates a post with tags, **Then** the post appears with those tags. Unknown tag names fail with a list of the valid tags.
3. **Given** a thread, **When** the agent archives, locks, renames or adds/removes members, **Then** the thread reflects the change.
4. **Given** a poll request with a question, up to 10 answers and a duration, **When** sent, **Then** the poll appears. The agent can later read per-answer counts and end the poll early.
5. **Given** message content read back from threads, forums or polls, **When** returned, **Then** it is marked as untrusted user content, as in v1.

---

### User Story 3 - Events, invites, voice and stage (Priority: P3)

An organizer asks: "schedule a game night on Friday at 8pm in the Lobby voice channel", "list
upcoming events and who's interested", "create a 7-day invite for #welcome limited to 10 uses",
"list and revoke old invites", "start a stage called 'Town Hall' in the Stage channel", "move
these five people from Lobby to Meeting Room", "mute @noisy in voice".

**Why this priority**: Events and invites drive community growth. Voice actions are frequent
during live sessions but matter less than moderation and content.

**Independent Test**: With the events, invites and voice groups enabled, perform each request
and verify it in Discord.

**Acceptance Scenarios**:

1. **Given** event details (name, time, location as a voice/stage channel or an external place), **When** created, **Then** the event appears. Times are interpreted in the time zone the user gives, or UTC if none, and are shown back with the zone.
2. **Given** existing events, **When** listed, **Then** each shows its status and interested count. Interested users can be listed in pages.
3. **Given** an invite request with a maximum age and use limit, **When** created, **Then** the invite link is returned. Revoking an invite requires confirmation.
4. **Given** named members who are in voice, **When** asked to move, mute, deafen or disconnect them, **Then** the change happens and the result names each affected member. Members not in voice are reported. (Finding everyone in a voice channel without being told needs a real-time connection, which is out of scope; see Assumptions.)
5. **Given** a stage channel, **When** asked to start or end a stage, **Then** the stage starts or ends with the given topic.

---

### User Story 4 - Auto-moderation rules (Priority: P4)

A server owner says "block invite links except in #partners", "flag messages with these words to
#mod-log", "time out anyone who mentions more than 10 people". The agent creates and adjusts
Discord's built-in auto-moderation rules.

**Why this priority**: Rules give lasting protection without a bot running, but they are set up
once and changed rarely.

**Independent Test**: With the automod group enabled, create, list, update and delete each kind
of rule on a test server, and confirm a matching message triggers the rule's action.

**Acceptance Scenarios**:

1. **Given** a rule description (trigger: keywords, preset lists, mention spam, spam; actions: block, alert to a channel, timeout; exemptions: roles, channels), **When** created, **Then** the rule exists with exactly those settings.
2. **Given** Discord's per-server limits on rule counts or keyword counts, **When** a request would exceed them, **Then** it is refused with the limit named.
3. **Given** a rule, **When** deleted or disabled, **Then** confirmation is required first.

---

### User Story 5 - Webhooks (Priority: P5)

A developer asks: "create a webhook in #deploys named 'CI' and give me the URL", "list all
webhooks and which channels they post to", "delete the webhooks nobody uses", "post this release
note through the CI webhook".

**Why this priority**: Useful for integrations, but used by fewer administrators.

**Independent Test**: With the webhooks group enabled, create, list, rename, move, post through
and delete a webhook on a test server.

**Acceptance Scenarios**:

1. **Given** a channel and a name, **When** a webhook is created, **Then** guildsmith returns its URL **only in that one response**, marked as a secret. Lists never show webhook URLs or tokens.
2. **Given** existing webhooks, **When** listed, **Then** each shows its name, channel, creator and kind (incoming, channel follower, application).
3. **Given** a webhook, **When** deleted, **Then** confirmation is required first.

---

### User Story 6 - Emoji, stickers and soundboard (Priority: P6)

A server owner asks: "upload these 5 images as emoji", "rename :old_logo: to :logo:", "add this
sticker", "list our custom emoji and how many slots are left", "add this sound to the
soundboard".

**Why this priority**: Nice for community identity, but not urgent.

**Independent Test**: With the expressions group enabled, upload, rename, list and delete emoji,
stickers and soundboard sounds on a test server.

**Acceptance Scenarios**:

1. **Given** an image file or URL and a name, **When** uploaded, **Then** the emoji appears. Files over Discord's size limit or in an unsupported format are refused before upload, with the limit named.
2. **Given** a server, **When** listing emoji, **Then** the result shows how many slots are used and how many remain at the server's boost level.
3. **Given** an emoji, sticker or sound, **When** deleted, **Then** confirmation is required first.

---

### User Story 7 - Server settings, onboarding and templates (Priority: P7)

An owner asks: "set the server description and the system channel", "set up onboarding: ask new
members which games they play and give them roles", "edit the welcome screen", "create a template
of this server", "sync our template", "turn this public template into a guild.toml I can review".

**Why this priority**: Rarely changed and high impact when changed. Templates also connect to
v1's server-as-code workflow.

**Independent Test**: With the settings group enabled, change settings, onboarding and the
welcome screen on a test server, and create, sync and import a template.

**Acceptance Scenarios**:

1. **Given** a settings change, **When** previewed, **Then** it shows each field as old → new. Applying it requires confirmation.
2. **Given** onboarding prompts with options mapped to roles and channels, **When** applied, **Then** new members see those prompts. References to unknown roles or channels fail before any change.
3. **Given** a template code or link, **When** imported, **Then** guildsmith produces a spec file for the template's roles and channels that validates and can then be planned against an existing server.
4. **Given** a request to create a new server from a template, **When** made, **Then** guildsmith explains that bots can't create servers and offers to import the template into a spec instead.

---

### User Story 8 - Turn on the tools you need (Priority: P8)

An operator wants everything available ("do anything"), or only the groups for a specific job.
They can enable groups one by one, or enable all of them at once and accept the larger context
cost. The agent can also turn on a group itself in the middle of a task, so the default context
stays small until a tool is actually needed. Everyone can see what each group adds and which
bot permissions it needs.

**Why this priority**: It cuts across every other story but adds no new Discord capability. v1's
mechanism already does most of this.

**Independent Test**: Enable each group alone, then all of them together, and confirm the tool
list, the size report and the setup check change to match. Then start with only the defaults and
have the agent enable a group mid-session and use one of its tools.

**Acceptance Scenarios**:

1. **Given** the operator enables "all", **When** the agent lists tools, **Then** every group's tools appear, and startup reports the total tool count and the approximate context cost.
2. **Given** a group is enabled, **When** the setup check runs, **Then** it lists the bot permissions that group needs and which are missing.
3. **Given** no group is enabled beyond the default, **When** the agent lists tools, **Then** the list is the v1 default set plus the two tool-group tools.
4. **Given** a group is disabled, **When** the agent needs one of its tools, **Then** it can list the available groups (with tool counts, context cost and required permissions), enable the group, and use the group's tools in the same session.
5. **Given** the operator has blocked a group, **When** the agent tries to enable it, **Then** it is refused with the reason.
6. **Given** the agent enabled groups, **When** the operator checks, **Then** every enablement appears in guildsmith's log with the time and group name.
7. **Given** the agent's client cannot refresh its tool list mid-session, **When** a group is enabled, **Then** the agent can still call that group's tools.

---

### User Story 9 - Manage the whole server as code (Priority: P9)

An owner wants the whole lasting setup of their server in one reviewable file, not just roles
and channels:
- auto-moderation rules, forum tags, onboarding and the welcome screen;
- server settings, custom emoji, stickers and soundboard sounds;
- webhooks, standing invites and upcoming scheduled events;
- bot-posted "fixed" messages such as a rules post or a role-menu post.

They export the server, edit the file, review the plan and apply it, exactly as v1 does for
structure.

**Why this priority**: It makes the full configuration reviewable and repeatable, and lets one
server's setup be copied to another. It builds on all the earlier stories, so it comes after them.

**Independent Test**: Configure a test server with at least one item of every covered kind,
export it, apply the file to a second empty server, then plan both servers against the file.
Both plans must show no changes.

**Acceptance Scenarios**:

1. **Given** a server with items of every covered kind, **When** exported, **Then** the file contains all of them, and validating it reports no errors.
2. **Given** an exported file applied to another server, **When** planned again, **Then** there are no changes.
3. **Given** a webhook in the file, **When** applied, **Then** the webhook is created and its URL is shown once in the apply result. The file never contains webhook URLs or tokens.
4. **Given** an invite in the file that has expired on the server, **When** planned, **Then** the plan reports it as expired and recreates it only if the file asks for that (a new invite gets a new code).
5. **Given** a scheduled event in the file whose time has passed, **When** planned, **Then** it is reported as past and never recreated.
6. **Given** emoji, stickers or sounds in the file that point to image or audio files, **When** a file's content changes, **Then** the plan shows a replacement for that item. Unchanged files produce no change.
7. **Given** a fixed message in the file, **When** its text changes, **Then** the plan edits the existing message in place, as long as the bot posted it. A message posted by someone else is reported as unmanageable.
8. **Given** items of the new kinds on the server but not in the file, **When** planned, **Then** they are reported as unmanaged and deleted only if pruning is turned on for that kind.

---

### User Story 10 - Raw access for anything else (Priority: P10)

An administrator needs something no dedicated tool covers yet, for example a brand-new Discord
feature. The agent sends a request directly to Discord's API, within the same safety limits:
only allowed servers, never in read-only mode for writes, and confirmation before any write.

**Why this priority**: A last resort that makes "do anything" literally true. Dedicated tools stay
the preferred path because they are safer and more compact.

**Independent Test**: With the raw group enabled, read an endpoint that no dedicated tool covers,
then make a write with and without confirmation, and against a server that is not allowed.

**Acceptance Scenarios**:

1. **Given** a read request for an allowed server, **When** sent, **Then** the response is returned, trimmed to the size limit and with user content marked untrusted.
2. **Given** a write request without confirmation, **When** sent, **Then** guildsmith shows what would be sent (method, path, body) and does nothing.
3. **Given** a request that targets a server not on the allowlist, or a resource guildsmith can't link to an allowed server, **When** sent, **Then** it is refused.
4. **Given** read-only mode is on, **When** any write is sent, **Then** it is refused.
5. **Given** a confirmed write, **When** sent, **Then** it carries an audit reason and the snapshot cache is cleared.
6. **Given** any request or response, **When** logged or returned, **Then** it never contains the bot token or webhook tokens.

---

### Edge Cases

- **Message age limits**: bulk deletion only works for messages newer than 14 days. Older messages are reported and offered for one-by-one deletion (slow, needs confirmation) rather than skipped silently.
- **Hierarchy**: banning, kicking, timing out or moving a member whose highest role is at or above the bot's fails for that member with a clear reason; the rest of a bulk action continues.
- **Owner and self**: the server owner and the bot itself can never be banned, kicked or timed out.
- **Bulk limits**: bulk ban takes at most 200 members per request. Larger lists are split into batches, each previewed with the total.
- **Server feature requirements**: some actions need a server feature (announcement channels, extra emoji slots, onboarding requires community mode). Requests fail early and name the missing feature.
- **Forum tag rules**: a forum that requires tags refuses posts without a tag. Posts use the forum's existing tags; new tags are created only on request.
- **Expired or used-up invites, ended events, archived threads**: shown with their state, never silently hidden.
- **Time zones**: event times without a zone are treated as UTC and shown back with "UTC".
- **Webhook secrets**: a webhook URL works as a credential. It appears only in the creation response, never in lists, logs or errors.
- **Files**: uploads (emoji, stickers, sounds, attachments) check type and size before sending. Local file paths are read only when the operator allows it (see Assumptions).
- **Rate limits on bulk actions**: long-running actions (mass role assignment, pruning, deleting many messages) report progress and can be stopped partway with an exact report of what was done.
- **Unsupported by bots**: anything Discord doesn't allow bots to do (creating servers, acting as a user) is refused with an explanation, never attempted.

## Requirements *(mandatory)*

### Functional Requirements

**Cross-cutting (applies to every new group)**

- **FR-001**: Every new tool in this feature MUST be in an **opt-in tool group**. The default tool set MUST stay as defined by 001-guildsmith-v1, except for the two tool-group tools required by FR-003a.
- **FR-002**: The system MUST provide these groups, each usable on its own: `moderation` (extended), `messages`, `threads` (threads, forums and tags), `polls`, `events`, `invites`, `voice` (voice and stage), `automod`, `webhooks`, `expressions` (emoji, stickers, soundboard), `settings` (server settings, onboarding, welcome screen, templates), `raw` (direct requests).
- **FR-003**: Operators MUST be able to enable any combination of groups at startup, or all of them at once with `all`.
- **FR-003a**: The agent MUST be able to list all groups (name, purpose, tool count, approximate context cost, required bot permissions, enabled or not) and enable any group during a session. It MUST be able to use the newly enabled tools in the same session, even if its client can't refresh its tool list.
- **FR-003b**: Operators MAY block specific groups. The agent MUST NOT be able to enable a blocked group, nor change the allowlist, read-only mode or the block list.
- **FR-003c**: Every group the agent enables MUST be logged with the time and group name.
- **FR-004**: Every new tool MUST follow v1's safety policy without exception: allowlist, read-only mode, a preview unless confirmation is given for anything destructive or hard to undo, an audit-log reason on every change, and permission and role-hierarchy checks before acting.
- **FR-005**: Bulk actions MUST preview the exact list or count before running, report progress, stop cleanly when interrupted, and report exactly which items succeeded, failed (with reason) or were not attempted.
- **FR-006**: Every new tool MUST be labeled read-only or destructive, follow v1's compact-output and paging rules, and never return raw Discord responses.
- **FR-007**: All user-written content returned by any new tool (messages, thread names, forum posts, poll answers, event descriptions, member names) MUST be marked as untrusted data.
- **FR-008**: Every new action MUST also be reachable from the command line, with the same behavior and policy as the agent tool.
- **FR-009**: The setup check MUST list the bot permissions each enabled group needs and report missing ones.
- **FR-010**: The system MUST report the tool count and approximate context cost of the enabled groups at startup and in the setup check.

**Moderation (extended)**

- **FR-011**: Ban and unban members, singly or in bulk (up to 200 per batch, larger lists batched), optionally deleting their recent messages (up to 7 days).
- **FR-012**: List bans with reasons, in pages.
- **FR-013**: Time out and clear timeouts (up to 28 days); kick members.
- **FR-014**: Prune inactive members (by days inactive, optionally limited to roles), with a preview count first.
- **FR-015**: Delete messages in bulk by channel, author, count or time range, within Discord's 14-day bulk limit; older messages are reported (see edge cases).
- **FR-016**: Add or remove a role for many members at once (bulk role assignment).

**Messages**

- **FR-017**: Edit, delete, pin, unpin and crosspost (publish announcement) messages; list pinned messages.
- **FR-018**: Add and remove reactions; list who reacted, in pages.
- **FR-019**: Send messages with embeds, attachments and replies, and with mentions limited to what the request explicitly allows (no accidental `@everyone`).
- **FR-020**: Search messages in a channel or server by text, author, time and has-attachment, where Discord makes search available to bots; otherwise fall back to scanning recent history within a stated limit.

**Threads and forums**

- **FR-021**: Create, rename, archive, unarchive, lock and delete threads; add and remove thread members; list active and archived threads.
- **FR-022**: Create forum and media posts with tags; edit a post's tags.
- **FR-023**: Create, rename and delete a forum's available tags and set its default reaction. These are also covered by the spec file (FR-038).

**Polls**

- **FR-024**: Create polls (question, up to 10 answers, duration up to 32 days, single or multiple choice), read per-answer counts and voters, and end polls early.

**Events, invites, voice and stage**

- **FR-025**: Create, edit, cancel and list scheduled events (voice, stage or external location; start and end time with time zone); list interested users.
- **FR-026**: Create invites (channel, maximum age, maximum uses, temporary membership), list invites with use counts, and revoke them.
- **FR-027**: Move, mute, deafen and disconnect members in voice; start, edit and end stage instances.

**Auto-moderation**

- **FR-028**: Create, list, update, enable/disable and delete auto-moderation rules, covering all trigger types, actions and exemptions Discord supports for bots.

**Webhooks**

- **FR-029**: Create, list, rename, move and delete webhooks; send messages through a webhook guildsmith created or was given.
- **FR-030**: Webhook URLs and tokens MUST be treated as secrets: shown only in the creation response, never in lists, logs or errors.

**Expressions**

- **FR-031**: Upload, rename, list and delete custom emoji, stickers and soundboard sounds, showing remaining slots per type for the server's boost level.

**Settings, onboarding, templates**

- **FR-032**: View and change server settings (name, description, icon, banner, system and rules channels, verification level, notification defaults, content filter, AFK channel and timeout), showing old → new and requiring confirmation.
- **FR-033**: View and change onboarding prompts and the welcome screen.
- **FR-034**: List, create, update, sync and delete the server's templates.
- **FR-035**: Import a template (by code or link) into a spec file that passes validation and can be planned against an existing server.

**Raw requests (`raw` group)**

- **FR-036**: The system MUST offer a tool that sends any request to Discord's API (method, path, query, body) as the bot, and returns the response.
- **FR-036a**: Raw requests MUST obey the allowlist. Requests whose target can't be tied to an allowed server MUST be refused. Requests for actions bots may not perform (such as creating servers) MUST be refused with an explanation.
- **FR-036b**: Raw writes (any method other than reading) MUST show a preview (method, path, body) and do nothing unless confirmed, MUST be refused in read-only mode, MUST carry an audit reason, and MUST clear cached server data.
- **FR-036c**: Raw responses MUST be trimmed to the result size limit, with user content marked untrusted. The bot token and webhook tokens MUST never appear in requests shown, responses or logs.
- **FR-036d**: Raw request descriptions MUST point the agent to a dedicated tool when one exists for the same action.

**Spec file coverage (server as code)**

- **FR-037**: The spec file MUST be able to describe, export, plan and apply these kinds in addition to v1's roles, categories, channels and overwrites:
  - server settings (this lifts v1's "informational only" rule when the file opts in);
  - auto-moderation rules;
  - forum and media tags and default reactions;
  - onboarding prompts and the welcome screen;
  - custom emoji, stickers and soundboard sounds;
  - webhooks;
  - standing invites;
  - upcoming scheduled events;
  - fixed messages posted by the bot, including their pinned state.
- **FR-038**: Every new kind MUST follow v1's plan rules:
  - items are matched by id, then by name within scope;
  - only items listed in the file are managed, and other items are reported as unmanaged;
  - deletion happens only with pruning on for that kind, and pruning is off by default;
  - running plan after apply shows no changes.
- **FR-039**: The spec file MUST NOT contain secrets. Webhook URLs and tokens are never exported or stored. A newly created webhook's URL is shown once in the apply result.
- **FR-040**: Emoji, stickers and sounds MUST refer to their image or audio by file path or URL. Plans MUST detect content changes and show them as a replacement.
- **FR-041**: Invites and scheduled events, which expire or end, MUST be reported as expired or past rather than silently recreated. An expired invite is recreated only when the file asks for it.
- **FR-042**: Fixed messages MUST be editable only if the bot posted them. Plans MUST report messages that can't be managed.
- **FR-043**: Items that only exist briefly or belong to members MUST stay out of the spec file: bans, member roles, ordinary messages, threads, polls, voice states and stage instances.
- **FR-044**: Templates imported under FR-035 MUST produce the same spec-file format.

### Key Entities

- **Tool group**: A named, opt-in set of tools with a list of the bot permissions it needs and its approximate context cost.
- **Bulk action**: A previewable action over many targets, with progress and a per-item outcome (succeeded, failed with reason, not attempted).
- **Ban**: A banned user, with reason and time.
- **Thread / forum post**: Parent channel, name, tags, archived/locked state, members.
- **Poll**: Question, answers, duration, choice mode, per-answer counts.
- **Scheduled event**: Name, description, location (channel or external), start/end time, status, interested users.
- **Invite**: Code, channel, creator, maximum age and uses, current uses, expiry.
- **Auto-moderation rule**: Trigger (type and settings), actions, exempt roles and channels, enabled state.
- **Webhook**: Name, channel, kind, creator; its URL is a secret.
- **Expression**: Emoji, sticker or sound, with name, kind and slot usage.
- **Server settings / onboarding / welcome screen / template**: Server-level configuration that changes rarely.
- **Fixed message**: A message posted by the bot whose text, embeds and pinned state are managed in the spec file.
- **Raw request**: Method, path, query and body, plus the allowed server it targets and whether it writes.
- **Group enablement**: The time, the group, and who enabled it (the operator at startup, or the agent during a session).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Every capability in the Context table that other Discord MCP servers offer, and that Discord allows bots to perform, is available in guildsmith.
- **SC-002**: With no extra groups enabled, the tool list is v1's default plus the two tool-group tools (listing/enabling groups and calling a tool in an enabled group). It stays within v1's limits of 25 tools and about 3,000 tokens of descriptions.
- **SC-003**: Each optional group adds at most 15 tools and at most about 1,500 tokens of agent context. Enabling all groups stays under about 15,000 tokens.
- **SC-004**: 100% of destructive or hard-to-undo actions in the new groups show a preview and change nothing without confirmation.
- **SC-005**: In bulk actions, 100% of targets end up in the outcome report as succeeded, failed with a reason, or not attempted.
- **SC-006**: Webhook URLs and tokens appear in zero lists, logs or error messages across the test suite.
- **SC-007**: An agent with all groups enabled completes a scripted set of 20 representative tasks (at least 2 from each user story) with no failed tool choice caused by unclear or overlapping tool descriptions.
- **SC-008**: A moderator can ban a list of 50 raid accounts, from asking to finished, in under 2 minutes, including reviewing the preview.
- **SC-009**: Importing a public template produces a spec file that passes validation in 100% of tested templates.
- **SC-010**: For a server with every spec-covered kind, exporting and applying to an empty server, then planning again, shows no changes on both servers.
- **SC-011**: Exported spec files contain zero webhook URLs or tokens.
- **SC-012**: 100% of raw writes are refused without confirmation, in read-only mode, or against servers outside the allowlist.
- **SC-013**: Starting with only the default tools, an agent can enable a group and complete a task with it in the same session, with no restart.

## Assumptions

- **Depends on v1**: This feature starts after 001-guildsmith-v1's core, safety policy and tool group mechanism exist. It replaces v1's decision to ship only a 4-tool moderation group (v1 research R20): that group becomes the full moderation group defined here.
- **REST only**: Like v1, everything uses Discord's request API. Real-time features (live event streams, reacting as messages arrive) stay out of scope.
- **Bot-only**: Only what Discord lets bots do. Creating servers, acting as a user and reading users' DMs are out of scope.
- **"Anything" means anything a bot can do**: Coverage is measured against Discord's bot-accessible features, not against what a human user can do in the app.
- **Local files**: Tools that upload files can read local paths only inside directories the operator allows; otherwise they accept URLs or inline data.
- **Default stays lean**: The constitution's budget for the default tool set is unchanged. Optional groups are measured, and their size is reported, but they are not capped beyond SC-003.
- **Search**: Message search depends on what Discord offers bots at implementation time; FR-020 defines the fallback.
- **Spec format grows, but stays compatible**: New kinds are optional sections. A v1 file stays valid and means the same thing.
- **Self-enabling is the operator's choice**: The agent may enable any group unless the operator blocks it. Operator settings (allowlist, read-only mode, block list) can never be changed by the agent. Read-only mode still blocks every write, including raw writes.
- **Constitution exception**: The raw group returns Discord's responses in raw form. This conflicts with constitution principle IV ("never raw Discord JSON"). It must be justified in the plan's Complexity Tracking, or the constitution amended, before implementation. Every dedicated tool still follows principle IV.
- **Voice presence**: Discord's request API can't list who is in a voice channel. Voice actions apply to members the agent names. "Move everyone in a channel" waits for the real-time connection milestone.
- **Client support**: How the agent sees newly enabled tools depends on its client. FR-003a requires a way that works even when the client can't refresh its tool list.
