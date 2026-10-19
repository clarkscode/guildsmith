//! Channels and categories, including kinds a spec can't describe (kept as `Other`).

use guildsmith_core::snapshot::{LiveChannel, LiveChannelKind, LiveTag};
use guildsmith_core::spec::ChannelKind;
use twilight_model::channel::{Channel, ChannelType};

use crate::ids;

pub(crate) fn kind(t: ChannelType) -> LiveChannelKind {
    match t {
        ChannelType::GuildCategory => LiveChannelKind::Category,
        ChannelType::GuildText => LiveChannelKind::Known(ChannelKind::Text),
        ChannelType::GuildAnnouncement => LiveChannelKind::Known(ChannelKind::Announcement),
        ChannelType::GuildVoice => LiveChannelKind::Known(ChannelKind::Voice),
        ChannelType::GuildStageVoice => LiveChannelKind::Known(ChannelKind::Stage),
        ChannelType::GuildForum => LiveChannelKind::Known(ChannelKind::Forum),
        ChannelType::GuildMedia => LiveChannelKind::Known(ChannelKind::Media),
        other => LiveChannelKind::Other(u8::from(other)),
    }
}

pub(crate) fn to_twilight_kind(k: LiveChannelKind) -> ChannelType {
    match k {
        LiveChannelKind::Category => ChannelType::GuildCategory,
        LiveChannelKind::Known(ChannelKind::Text) => ChannelType::GuildText,
        LiveChannelKind::Known(ChannelKind::Announcement) => ChannelType::GuildAnnouncement,
        LiveChannelKind::Known(ChannelKind::Voice) => ChannelType::GuildVoice,
        LiveChannelKind::Known(ChannelKind::Stage) => ChannelType::GuildStageVoice,
        LiveChannelKind::Known(ChannelKind::Forum) => ChannelType::GuildForum,
        LiveChannelKind::Known(ChannelKind::Media) => ChannelType::GuildMedia,
        LiveChannelKind::Other(n) => ChannelType::from(n),
    }
}

fn emoji(
    id: Option<twilight_model::id::Id<twilight_model::id::marker::EmojiMarker>>,
    name: Option<&String>,
) -> Option<String> {
    name.cloned().or_else(|| id.map(|i| i.get().to_string()))
}

pub(crate) fn live(c: &Channel) -> LiveChannel {
    LiveChannel {
        id: ids::from(c.id),
        name: c.name.clone().unwrap_or_default(),
        kind: kind(c.kind),
        parent_id: c.parent_id.map(ids::from),
        position: c.position.unwrap_or(0),
        topic: c.topic.clone().filter(|t| !t.is_empty()),
        nsfw: c.nsfw.unwrap_or(false),
        slowmode: c.rate_limit_per_user.map_or(0, u32::from),
        bitrate: c.bitrate,
        user_limit: c.user_limit,
        rtc_region: c.rtc_region.clone(),
        tags: c
            .available_tags
            .iter()
            .flatten()
            .map(|t| LiveTag {
                id: ids::from(t.id),
                name: t.name.clone(),
                emoji: emoji(t.emoji_id, t.emoji_name.as_ref()),
                moderated: t.moderated,
            })
            .collect(),
        default_reaction: c.default_reaction_emoji.as_ref().and_then(|r| emoji(r.emoji_id, r.emoji_name.as_ref())),
        overwrites: c.permission_overwrites.iter().flatten().filter_map(super::overwrite::live).collect(),
    }
}
