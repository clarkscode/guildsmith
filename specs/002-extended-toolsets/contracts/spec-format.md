# Contract: spec file additions

Extends [001 spec-format.md](../../001-guildsmith-v1/contracts/spec-format.md). Field rules
are in [../data-model.md](../data-model.md) §4. Every new section is optional, so a v1 file is
still valid and means the same thing.

## Example

```toml
[guild]
name = "Guildsmith Lab"
manage_settings = true            # turns on [guild.settings]

[guild.settings]
description = "A lab for guildsmith"
icon = { file = "assets/guild/icon.png" }
rules_channel = "#rules"
system_channel = "#welcome"
verification_level = "medium"
features = { community = true }

[[automod]]
name = "No invite links"
trigger = { keyword = { regex = ['discord\.gg/\w+'] } }
actions = [{ block = { message = "Invites aren't allowed here." } }, { alert = { channel = "#mod-log" } }]
exempt_channels = ["#partners"]

[onboarding]
enabled = true
mode = "advanced"
default_channels = ["#welcome", "#rules", "#general", "#announcements", "#help", "#off-topic", "#events"]

  [[onboarding.prompts]]
  title = "Which games do you play?"
  kind = "multiple_choice"
  options = [
    { title = "Minecraft", roles = ["@Minecraft"] },
    { title = "Valorant", roles = ["@Valorant"], channels = ["#valorant"] },
  ]

[[emoji]]
name = "guildsmith"
image = { file = "assets/emoji/guildsmith.png" }

[[webhooks]]                      # no token or url, ever
name = "CI"
channel = "#deploys"

[[invites]]                       # without a code, matched by signature
channel = "#welcome"
max_age = 0                       # never expires
max_uses = 0
roles = ["@Member"]

[[events]]
name = "Game night"
start = 2026-10-23T20:00:00-07:00
location = { voice = "Lobby" }
recurrence = { frequency = "weekly", weekday = "friday" }

[[messages]]                      # a fixed rules post
id = "623456789012345678"         # written back by apply (E9)
channel = "#rules"
pinned = true
content = """
1. Be kind.
2. No spam.
"""

[prune]
automod = false
emoji = false
webhooks = false
invites = false
events = false
messages = false
```

## Rules

- **References** (`#channel`, `@Role`) resolve against the same file first, then against the live
  server. An unresolved reference is a validation error.
- **Assets**: `file` paths are relative to the spec file. Over MCP they are read only under
  `GUILDSMITH_FILE_ROOTS`. `url` assets are fetched at plan time.
- **Secrets are rejected**: any value shaped like a webhook URL or token fails validation (E8).
- **Not allowed** (FR-043): sections for bans, members, member roles, threads, polls, voice
  states, stage instances or ordinary messages are unknown keys, which are errors.
- **Time values**: `start`/`end` are TOML datetimes with a UTC offset. A local datetime without
  an offset is an error, so no time zone is ever guessed.
- **Ids written back**: `apply --write-ids` adds only `id = "…"` (or `code = "…"` for invites)
  lines. Everything else, comments included, stays the same.
