//! Reading what a spec's extra kinds need (US9): the live state of each kind the spec lists,
//! the blake3 of live files on Discord's CDN (only for kinds the spec lists, each URL read once
//! per run), and the blake3 of the spec's own files (research E7).

use std::collections::BTreeMap;

use guildsmith_core::Id;
use guildsmith_core::asset::{self, Asset, AssetKind};
use guildsmith_core::live::{
    ExpressionKind, LiveExtras, LiveMessage, asset_key, avatar_url, event_cover_url, guild_image_url,
};
use guildsmith_core::planner::SpecAssets;
use guildsmith_core::snapshot::Snapshot;
use guildsmith_core::spec::{GuildSpec, SpecPath};

use crate::App;
use crate::assets::{self, Caller};
use crate::date;
use crate::error::ServiceError;
use crate::ports::{DiscordPort, MessageInfo};
use crate::resolve::{self, Resolution};

/// Every file the spec names, with its kind and spec path.
fn spec_files(spec: &GuildSpec) -> Vec<(SpecPath, &Asset, AssetKind)> {
    let root = SpecPath::root;
    let mut out = Vec::new();
    if let Some(s) = spec.guild.settings.as_ref().filter(|_| spec.guild.manage_settings) {
        for (f, a) in [("icon", &s.icon), ("banner", &s.banner), ("splash", &s.splash)] {
            if let Some(a) = a.as_ref().filter(|a| !matches!(a, Asset::Data { data } if data.is_empty())) {
                out.push((root().key("guild").key("settings").key(f), a, AssetKind::GuildImage));
            }
        }
    }
    for (i, e) in spec.emoji.iter().enumerate() {
        out.push((root().key("emoji").index(i).key("image"), &e.image, AssetKind::Emoji));
    }
    for (i, s) in spec.stickers.iter().enumerate() {
        out.push((root().key("stickers").index(i).key("image"), &s.image, AssetKind::Sticker));
    }
    for (i, s) in spec.sounds.iter().enumerate() {
        out.push((root().key("sounds").index(i).key("sound"), &s.sound, AssetKind::Sound));
    }
    for (i, w) in spec.webhooks.iter().enumerate() {
        if let Some(a) = &w.avatar {
            out.push((root().key("webhooks").index(i).key("avatar"), a, AssetKind::Avatar));
        }
    }
    for (i, e) in spec.events.iter().enumerate() {
        if let Some(a) = &e.image {
            out.push((root().key("events").index(i).key("image"), a, AssetKind::GuildImage));
        }
    }
    out
}

/// The blake3 of every file the spec names, by spec path.
pub(crate) async fn spec_assets<P: DiscordPort>(
    app: &App<P>,
    spec: &GuildSpec,
    caller: Caller<'_>,
) -> Result<SpecAssets, ServiceError> {
    let mut out = SpecAssets::new();
    let mut problems = Vec::new();
    for (path, a, kind) in spec_files(spec) {
        match assets::load(&app.port, &app.policy, caller, a, kind).await {
            Ok(l) => {
                out.insert(path.to_string(), l.hash);
            }
            Err(e) => problems.push(format!("{path}: {e}")),
        }
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(ServiceError::Invalid(format!("the spec's files can't be used:\n- {}", problems.join("\n- "))))
    }
}

/// Live files, read once per URL for the run.
struct Cdn<'a, P> {
    app: &'a App<P>,
    seen: BTreeMap<String, Option<String>>,
}

impl<P: DiscordPort> Cdn<'_, P> {
    async fn hash(&mut self, url: &str, kind: AssetKind) -> Option<String> {
        if let Some(h) = self.seen.get(url) {
            return h.clone();
        }
        let h = match self.app.port.fetch_url(url, asset::limits(kind).max_bytes + 1).await {
            Ok(bytes) => Some(asset::hash(&bytes)),
            Err(e) => {
                tracing::warn!(url, error = %e, "couldn't read a live file to compare");
                None
            }
        };
        self.seen.insert(url.to_owned(), h.clone());
        h
    }
}

fn message(m: &MessageInfo) -> LiveMessage {
    LiveMessage {
        id: m.id,
        content: m.content.clone(),
        embeds: m.embeds.clone(),
        components: m.components.clone(),
        pinned: m.pinned,
        flags: m.flags,
    }
}

/// The bot's own messages in each channel the spec's fixed messages use.
async fn messages<P: DiscordPort>(
    app: &App<P>,
    s: &Snapshot,
    spec: &GuildSpec,
) -> Result<BTreeMap<Id, Vec<LiveMessage>>, ServiceError> {
    let bot = s.bot.user_id;
    let mut out: BTreeMap<Id, Vec<LiveMessage>> = BTreeMap::new();
    for m in &spec.messages {
        // Channels the plan creates have no messages yet.
        let Resolution::Resolved(cid) = resolve::resolve_channel(s, &m.channel) else { continue };
        if let std::collections::btree_map::Entry::Vacant(slot) = out.entry(cid) {
            let recent = app.port.recent(cid, 100, None).await?;
            slot.insert(recent.iter().filter(|x| x.author_id == bot).map(message).collect());
        }
        if let Some(id) = m.id
            && !out[&cid].iter().any(|x| x.id == id)
            && let Ok(found) = app.port.message(cid, id).await
            && found.author_id == bot
        {
            out.entry(cid).or_default().push(message(&found));
        }
    }
    Ok(out)
}

/// The live state of every kind, for export: as if a spec listed one of each (fixed messages
/// aside: they're the bot's own posts, which export can't tell from ordinary ones). A kind the
/// bot can't read is left out, with a warning.
pub(crate) async fn read_all<P: DiscordPort>(app: &App<P>, gid: Id, s: &Snapshot) -> (LiveExtras, Vec<String>) {
    let one = |f: &dyn Fn(&mut GuildSpec)| {
        let mut spec = GuildSpec::default();
        f(&mut spec);
        spec
    };
    let kinds: [(&str, GuildSpec); 6] = [
        (
            "server settings",
            one(&|s| {
                s.guild.manage_settings = true;
                s.guild.settings = Some(guildsmith_core::spec::settings::SettingsSpec::default());
            }),
        ),
        ("automod rules", one(&|s| s.prune.automod = true)),
        (
            "emoji, stickers and sounds",
            one(&|s| {
                s.prune.emoji = true;
                s.prune.stickers = true;
                s.prune.sounds = true;
            }),
        ),
        ("webhooks", one(&|s| s.prune.webhooks = true)),
        ("invites", one(&|s| s.prune.invites = true)),
        ("events", one(&|s| s.prune.events = true)),
    ];
    let mut x = LiveExtras { now: date::now_secs(app.clock.as_ref()), ..LiveExtras::default() };
    let mut warnings = Vec::new();
    for (label, spec) in kinds {
        match read(app, gid, s, &spec).await {
            Ok(part) => {
                x.settings = x.settings.or(part.settings);
                x.automod = x.automod.or(part.automod);
                x.expressions = x.expressions.or(part.expressions);
                x.webhooks = x.webhooks.or(part.webhooks);
                x.invites = x.invites.or(part.invites);
                x.events = x.events.or(part.events);
                x.asset_hashes.extend(part.asset_hashes);
            }
            Err(e) => warnings.push(format!("{label} not exported: {e}")),
        }
    }
    // Servers without Community have no onboarding or welcome screen to read.
    x.onboarding = app.port.onboarding(gid).await.ok();
    x.welcome_screen = app.port.welcome_screen(gid).await.ok();
    (x, warnings)
}

/// The live state of every kind `spec` lists, with live file hashes for the kinds whose files
/// it names.
#[allow(clippy::too_many_lines)] // one block per kind
pub(crate) async fn read<P: DiscordPort>(
    app: &App<P>,
    gid: Id,
    s: &Snapshot,
    spec: &GuildSpec,
) -> Result<LiveExtras, ServiceError> {
    let p = &spec.prune;
    let mut x = LiveExtras { now: date::now_secs(app.clock.as_ref()), ..LiveExtras::default() };
    let mut cdn = Cdn { app, seen: BTreeMap::new() };
    if let Some(want) = spec.guild.settings.as_ref().filter(|_| spec.guild.manage_settings) {
        let live = app.port.guild_settings(gid).await?;
        for (field, folder, wanted, have) in [
            ("icon", "icons", &want.icon, &live.icon),
            ("banner", "banners", &want.banner, &live.banner),
            ("splash", "splashes", &want.splash, &live.splash),
        ] {
            if let (Some(_), Some(h)) = (wanted, have)
                && let Some(hash) = cdn.hash(&guild_image_url(folder, gid, h), AssetKind::GuildImage).await
            {
                x.asset_hashes.insert(asset_key("settings", field), hash);
            }
        }
        x.settings = Some(live);
    }
    if !spec.automod.is_empty() || p.automod {
        x.automod = Some(app.port.automod_rules(gid).await?);
    }
    if spec.onboarding.is_some() {
        x.onboarding = Some(app.port.onboarding(gid).await?);
    }
    if spec.welcome_screen.is_some() {
        x.welcome_screen = Some(app.port.welcome_screen(gid).await?);
    }
    let kinds = [
        (ExpressionKind::Emoji, !spec.emoji.is_empty(), p.emoji, AssetKind::Emoji),
        (ExpressionKind::Sticker, !spec.stickers.is_empty(), p.stickers, AssetKind::Sticker),
        (ExpressionKind::Sound, !spec.sounds.is_empty(), p.sounds, AssetKind::Sound),
    ];
    if kinds.iter().any(|(_, listed, prune, _)| *listed || *prune) {
        let mut all = Vec::new();
        for (kind, listed, prune, asset_kind) in kinds {
            if !(listed || prune) {
                continue;
            }
            let list = app.port.expressions(gid, kind).await?;
            if listed {
                for e in &list {
                    if let Some(h) = cdn.hash(&e.url, asset_kind).await {
                        x.asset_hashes.insert(asset_key(kind.name(), e.id), h);
                    }
                }
            }
            all.extend(list);
        }
        x.expressions = Some(all);
    }
    if !spec.webhooks.is_empty() || p.webhooks {
        let list = app.port.guild_webhooks(gid).await?;
        if spec.webhooks.iter().any(|w| w.avatar.is_some()) {
            for w in &list {
                if let Some(a) = &w.avatar
                    && let Some(h) = cdn.hash(&avatar_url(w.id, a), AssetKind::Avatar).await
                {
                    x.asset_hashes.insert(asset_key("webhook", w.id), h);
                }
            }
        }
        x.webhooks = Some(list);
    }
    if !spec.invites.is_empty() || p.invites {
        x.invites = Some(app.port.guild_invites(gid).await?);
    }
    if !spec.events.is_empty() || p.events {
        let list = app.port.events(gid).await?;
        if spec.events.iter().any(|e| e.image.is_some()) {
            for e in &list {
                if let Some(i) = &e.image
                    && let Some(h) = cdn.hash(&event_cover_url(e.id, i), AssetKind::GuildImage).await
                {
                    x.asset_hashes.insert(asset_key("event", e.id), h);
                }
            }
        }
        x.events = Some(list);
    }
    if !spec.messages.is_empty() {
        x.messages = Some(messages(app, s, spec).await?);
    }
    Ok(x)
}
