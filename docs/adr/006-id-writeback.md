# ADR-006: Writing created ids back into the spec file

- Status: accepted
- Date: 2026-10-20
- Context: spec 002 US9 and FR-037–042; research E9; data-model §5 `WriteIds`

## Decision

- `guildsmith apply --write-ids` (on by default when the CLI is given a file path; turn it off
  with `--no-write-ids`) and `spec_apply` with a `path` write the ids of newly created items
  into the user's spec file.
- Only `id = "…"` lines are added (`code = "…"` for invites). The edit goes through
  `toml_edit`, so comments, ordering and formatting elsewhere in the file are kept byte for
  byte. A second apply with nothing new to create writes nothing.
- This replaces v1's "never rewrite the user's file" rule, but only for adding ids.

## Why

Some kinds have no other natural key:

- a fixed message without an id can only be matched by its exact content, so editing its text
  would post a new message instead of editing the old one;
- an invite without a code is matched by its signature (channel, max age, max uses, temporary,
  roles), which breaks as soon as one of those changes.

Ids also make later renames of every kind safe, the same way v1 ids do for roles and channels.

## Consequences

- Ids are the identity of items the spec created, not a separate state file, so constitution
  III ("there is no separate state file") still holds: deleting the ids only falls back to
  name, signature or content matching.
- Webhook tokens and URLs are never written back (FR-039, research E8). A newly created
  webhook's URL appears once in the apply result instead.
- Writing happens after the apply finishes (`WriteIds` is the last op). If the apply stops
  partway, ids for the items that were created are still written, so the next plan matches
  them by id.
