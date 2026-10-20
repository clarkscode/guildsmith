//! `[[emoji]]`, `[[stickers]]` and `[[sounds]]` (data-model §4). Files are [`Asset`]s, compared
//! by blake3 of their bytes (research E7).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::asset::Asset;
use crate::id::Id;

/// A sound's volume, 0–1. Compared exactly, so it can sit in plans (`Eq`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Volume(pub f64);

// Validation keeps volumes in 0–1, so NaN never reaches a comparison.
impl Eq for Volume {}

fn name_ok(name: &str, max: usize, emoji: bool) -> bool {
    let n = name.chars().count();
    (2..=max).contains(&n) && (!emoji || name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// `[[emoji]]`: a custom emoji.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmojiSpec {
    /// Written back by apply. Without it, the emoji is matched by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 2–32 characters: letters, digits and `_`.
    pub name: String,
    /// PNG, JPEG, GIF, WebP or AVIF, at most 256 KiB: {file}, {url} or {data}.
    pub image: Asset,
    /// Roles allowed to use it (`@Role`); empty = everyone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
}

impl EmojiSpec {
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !name_ok(&self.name, 32, true) {
            out.push("emoji names are 2–32 characters of letters, digits and _".to_owned());
        }
        out
    }
}

/// `[[stickers]]`: a custom sticker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StickerSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 2–30 characters.
    pub name: String,
    /// Empty, or 2–100 characters.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Autocomplete tags (e.g. a related emoji name), at most 200 characters.
    pub tags: String,
    /// PNG, APNG, GIF or Lottie JSON, at most 512 KiB.
    pub image: Asset,
}

impl StickerSpec {
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !name_ok(&self.name, 30, false) {
            out.push("sticker names are 2–30 characters".to_owned());
        }
        let d = self.description.chars().count();
        if d == 1 || d > 100 {
            out.push("sticker descriptions are empty or 2–100 characters".to_owned());
        }
        let t = self.tags.chars().count();
        if t == 0 || t > 200 {
            out.push("sticker tags are 1–200 characters".to_owned());
        }
        out
    }
}

/// `[[sounds]]`: a soundboard sound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SoundSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// 2–32 characters.
    pub name: String,
    /// MP3 or Ogg, at most 512 KiB and 5.2 seconds.
    pub sound: Asset,
    /// 0–1; Discord's default is 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<Volume>,
    /// A unicode emoji or a custom emoji id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
}

impl SoundSpec {
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !name_ok(&self.name, 32, false) {
            out.push("sound names are 2–32 characters".to_owned());
        }
        if self.volume.is_some_and(|v| !(0.0..=1.0).contains(&v.0)) {
            out.push("volume is 0–1".to_owned());
        }
        out
    }
}
