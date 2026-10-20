# ADR-005: Raw request group, an exception to constitution IV

- Status: accepted (exception recorded; see "Gate" below)
- Date: 2026-10-20
- Context: spec 002 US10 and FR-036–036d; research E2; plan Complexity Tracking

## Decision

An opt-in `raw` group with one tool, `discord_request(method, path, query?, body?, confirm?,
reason?)`, sends any request to Discord's API as the bot.

- **Same HTTP stack.** It is sent with twilight's `RequestBuilder::raw` and `Client::request`,
  so it uses the same auth and the same rate limiter, keyed by method and path, as every other
  call. Raw calls can't get around the global limit.
- **Scoped to allowed servers.** A pure classifier in `guildsmith-core::raw` ties the path to a
  server: `guilds/{id}/…` directly; `channels/{id}/…`, `stage-instances/{channel}`,
  `webhooks/{id}` and `invites/{code}/…` through one lookup; `users/@me` (GET only) and
  `users/@me/guilds/{id}` (DELETE, the bot leaves; destructive). Anything that can't be tied to
  an allowed server is refused.
- **Refused outright:** token routes (`webhooks/{id}/{token}…`), `applications/…/commands`
  (guildsmith is not a bot framework), `oauth2/…`, `gateway…`, `POST guilds` (bots can't create
  servers), and any body that carries an `Authorization` header or a token.
- **Writes follow the policy.** Every method other than GET previews `{method, path, query,
  body}` until `confirm`, is refused in read-only mode, carries `X-Audit-Log-Reason`
  (`guildsmith: …`, percent-encoded), and clears the snapshot cache.
- **Responses are made safe.** Every `token` field is removed, webhook URLs become
  `"<redacted>"`, the body is cut to 50,000 characters with a note, and the whole response is
  wrapped as untrusted content. The tool description points the agent to a dedicated tool when
  one exists (FR-036d).

## The exception

Constitution IV says results are "never raw Discord JSON". `discord_request` returns Discord's
JSON, because only a raw response can describe an endpoint guildsmith has no typed model for
(for example, a Discord feature released after this version).

The exception is limited to this one tool:

- the `raw` group is opt-in, and the operator can block it with `GUILDSMITH_TOOLSETS_BLOCK=raw`;
- tokens are removed and webhook URLs redacted;
- output is cut at 50,000 characters and wrapped as untrusted data;
- every other tool still follows principle IV.

Alternatives considered: typed tools only (anything new would wait for a guildsmith release)
and a read-only raw tool (the user asked for writes, spec Q2 → C).

## Gate (T106)

The plan recommends a MINOR amendment of principle IV (1.0.0 → 1.1.0): "…never raw Discord JSON,
**except in the opt-in `raw` group, whose output is redacted, size-limited and wrapped as
untrusted**." Until that amendment is ratified, this ADR is the recorded, accepted exception.
Reviewer sign-off is recorded below.

| Reviewer | Decision | Date |
|---|---|---|
| Project owner | Amend the constitution instead of a standing exception: principle IV gains a scoped exception for the opt-in `raw` group (constitution 1.1.0). | 2026-10-20 |
