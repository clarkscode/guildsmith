//! Discord permission flags with names that read both ways (research R2 table).
//!
//! guildsmith owns this table so it does not depend on the Discord library's constants
//! (twilight 0.17.1 lacks `SET_VOICE_CHANNEL_STATUS`, bit 48). Unknown bits from Discord are
//! kept as `unknown_<bit>` so a round trip never loses data.

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! permissions {
    ($($variant:ident = $bit:literal => $name:literal,)+) => {
        /// One Discord permission flag.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Permission {
            $($variant,)+
        }

        impl Permission {
            /// Every known permission, in bit order.
            pub const ALL: &'static [Permission] = &[$(Permission::$variant,)+];

            /// The bit position of this flag.
            #[must_use]
            pub const fn bit(self) -> u8 {
                match self {
                    $(Permission::$variant => $bit,)+
                }
            }

            /// The `snake_case` name used in spec files and tool output.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Permission::$variant => $name,)+
                }
            }

            /// Looks a permission up by its `snake_case` name.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Permission::$variant),)+
                    _ => None,
                }
            }

            /// Looks a permission up by its bit position.
            #[must_use]
            pub const fn from_bit(bit: u8) -> Option<Self> {
                match bit {
                    $($bit => Some(Permission::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

permissions! {
    CreateInstantInvite = 0 => "create_instant_invite",
    KickMembers = 1 => "kick_members",
    BanMembers = 2 => "ban_members",
    Administrator = 3 => "administrator",
    ManageChannels = 4 => "manage_channels",
    ManageGuild = 5 => "manage_guild",
    AddReactions = 6 => "add_reactions",
    ViewAuditLog = 7 => "view_audit_log",
    PrioritySpeaker = 8 => "priority_speaker",
    Stream = 9 => "stream",
    ViewChannel = 10 => "view_channel",
    SendMessages = 11 => "send_messages",
    SendTtsMessages = 12 => "send_tts_messages",
    ManageMessages = 13 => "manage_messages",
    EmbedLinks = 14 => "embed_links",
    AttachFiles = 15 => "attach_files",
    ReadMessageHistory = 16 => "read_message_history",
    MentionEveryone = 17 => "mention_everyone",
    UseExternalEmojis = 18 => "use_external_emojis",
    ViewGuildInsights = 19 => "view_guild_insights",
    Connect = 20 => "connect",
    Speak = 21 => "speak",
    MuteMembers = 22 => "mute_members",
    DeafenMembers = 23 => "deafen_members",
    MoveMembers = 24 => "move_members",
    UseVad = 25 => "use_vad",
    ChangeNickname = 26 => "change_nickname",
    ManageNicknames = 27 => "manage_nicknames",
    ManageRoles = 28 => "manage_roles",
    ManageWebhooks = 29 => "manage_webhooks",
    ManageGuildExpressions = 30 => "manage_guild_expressions",
    UseApplicationCommands = 31 => "use_application_commands",
    RequestToSpeak = 32 => "request_to_speak",
    ManageEvents = 33 => "manage_events",
    ManageThreads = 34 => "manage_threads",
    CreatePublicThreads = 35 => "create_public_threads",
    CreatePrivateThreads = 36 => "create_private_threads",
    UseExternalStickers = 37 => "use_external_stickers",
    SendMessagesInThreads = 38 => "send_messages_in_threads",
    UseEmbeddedActivities = 39 => "use_embedded_activities",
    ModerateMembers = 40 => "moderate_members",
    ViewCreatorMonetizationAnalytics = 41 => "view_creator_monetization_analytics",
    UseSoundboard = 42 => "use_soundboard",
    CreateGuildExpressions = 43 => "create_guild_expressions",
    CreateEvents = 44 => "create_events",
    UseExternalSounds = 45 => "use_external_sounds",
    SendVoiceMessages = 46 => "send_voice_messages",
    SetVoiceChannelStatus = 48 => "set_voice_channel_status",
    SendPolls = 49 => "send_polls",
    UseExternalApps = 50 => "use_external_apps",
    PinMessages = 51 => "pin_messages",
    BypassSlowmode = 52 => "bypass_slowmode",
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A set of permission bits. Serializes as a list of names sorted alphabetically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PermissionSet(u64);

impl PermissionSet {
    pub const EMPTY: Self = PermissionSet(0);

    /// Builds a set from raw bits, keeping bits guildsmith does not know.
    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        PermissionSet(bits)
    }

    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Every known permission.
    #[must_use]
    pub fn all_known() -> Self {
        Permission::ALL.iter().copied().collect()
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn contains(self, p: Permission) -> bool {
        self.0 & (1 << p.bit()) != 0
    }

    pub fn insert(&mut self, p: Permission) {
        self.0 |= 1 << p.bit();
    }

    pub fn remove(&mut self, p: Permission) {
        self.0 &= !(1 << p.bit());
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        PermissionSet(self.0 | other.0)
    }

    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        PermissionSet(self.0 & other.0)
    }

    /// Bits in `self` that are not in `other`.
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        PermissionSet(self.0 & !other.0)
    }

    /// Known permissions in the set, in bit order.
    pub fn iter(self) -> impl Iterator<Item = Permission> {
        Permission::ALL.iter().copied().filter(move |p| self.contains(*p))
    }

    /// Bit positions set in `self` that have no known name.
    pub fn unknown_bits(self) -> impl Iterator<Item = u8> {
        (0u8..64).filter(move |b| self.0 & (1 << b) != 0 && Permission::from_bit(*b).is_none())
    }

    /// Names in the set, sorted alphabetically (the canonical serialized form).
    #[must_use]
    pub fn names(self) -> Vec<String> {
        let mut names: Vec<String> = self.iter().map(|p| p.name().to_owned()).collect();
        names.extend(self.unknown_bits().map(|b| format!("unknown_{b}")));
        names.sort();
        names
    }

    /// Parses Discord's decimal-string form (e.g. `"2147483648"`).
    pub fn from_discord_str(s: &str) -> Result<Self, ParsePermissionError> {
        s.parse::<u64>().map(PermissionSet).map_err(|_| ParsePermissionError::BadNumber(s.to_owned()))
    }

    /// Discord's decimal-string form.
    #[must_use]
    pub fn to_discord_string(self) -> String {
        self.0.to_string()
    }

    /// Parses one name: a known permission or `unknown_<bit>`.
    pub fn parse_name(name: &str) -> Result<u64, ParsePermissionError> {
        if let Some(p) = Permission::from_name(name) {
            return Ok(1 << p.bit());
        }
        if let Some(rest) = name.strip_prefix("unknown_")
            && let Ok(bit) = rest.parse::<u8>()
            && bit < 64
        {
            return Ok(1 << bit);
        }
        Err(ParsePermissionError::UnknownName(name.to_owned()))
    }
}

impl FromIterator<Permission> for PermissionSet {
    fn from_iter<I: IntoIterator<Item = Permission>>(iter: I) -> Self {
        let mut set = PermissionSet::EMPTY;
        for p in iter {
            set.insert(p);
        }
        set
    }
}

/// Errors parsing permissions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParsePermissionError {
    #[error("unknown permission `{0}`")]
    UnknownName(String),
    #[error("`{0}` is not a permission bit set (expected a decimal number)")]
    BadNumber(String),
}

impl FromStr for Permission {
    type Err = ParsePermissionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Permission::from_name(s).ok_or_else(|| ParsePermissionError::UnknownName(s.to_owned()))
    }
}

impl Serialize for PermissionSet {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.names())
    }
}

impl<'de> Deserialize<'de> for PermissionSet {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let names = Vec::<String>::deserialize(d)?;
        let mut bits = 0;
        for name in &names {
            bits |= PermissionSet::parse_name(name).map_err(serde::de::Error::custom)?;
        }
        Ok(PermissionSet(bits))
    }
}

impl JsonSchema for PermissionSet {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PermissionSet".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let names: Vec<&str> = Permission::ALL.iter().map(|p| p.name()).collect();
        schemars::json_schema!({
            "type": "array",
            "uniqueItems": true,
            "description": "Discord permission names in snake_case.",
            "items": {
                "anyOf": [
                    { "enum": names },
                    { "type": "string", "pattern": "^unknown_[0-9]{1,2}$" }
                ]
            }
        })
    }
}
