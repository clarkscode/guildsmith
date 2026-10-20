//! Assets: images and sounds for emoji, stickers, soundboard sounds, server images, webhook
//! avatars and message attachments (research E4/E7, data-model §4).
//!
//! Pure: the bytes are handed in (the app layer reads files, the adapter downloads from the
//! CDN). This module detects the file type, checks it against the kind's limits before any
//! upload, measures sound length, hashes bytes for plan comparison, and builds data URIs.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where an asset's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Asset {
    /// A local file, relative to the spec file (or a path under `GUILDSMITH_FILE_ROOTS`).
    File { file: String },
    /// An `http(s)` URL, fetched when needed.
    Url { url: String },
    /// Inline base64, or a `data:` URI.
    Data { data: String },
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Asset::File { file } => write!(f, "file {file}"),
            Asset::Url { url } => write!(f, "url {url}"),
            Asset::Data { data } => write!(f, "inline data ({} characters)", data.len()),
        }
    }
}

/// What an asset is for. Each kind has its own limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Emoji,
    Sticker,
    Sound,
    /// A message attachment.
    Attachment,
    /// Server icon, banner, splash or a scheduled event's cover image.
    GuildImage,
    /// A webhook's avatar.
    Avatar,
}

impl AssetKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            AssetKind::Emoji => "emoji",
            AssetKind::Sticker => "sticker",
            AssetKind::Sound => "sound",
            AssetKind::Attachment => "attachment",
            AssetKind::GuildImage => "server image",
            AssetKind::Avatar => "avatar",
        }
    }
}

/// A detected file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    Png,
    /// Animated PNG.
    Apng,
    Jpeg,
    Gif,
    Webp,
    Avif,
    Mp3,
    Ogg,
    /// Lottie JSON (stickers on verified or partnered servers only).
    Lottie,
    /// Something else; only attachments accept it.
    Other,
}

impl FileType {
    #[must_use]
    pub const fn mime(self) -> &'static str {
        match self {
            FileType::Png | FileType::Apng => "image/png",
            FileType::Jpeg => "image/jpeg",
            FileType::Gif => "image/gif",
            FileType::Webp => "image/webp",
            FileType::Avif => "image/avif",
            FileType::Mp3 => "audio/mpeg",
            FileType::Ogg => "audio/ogg",
            FileType::Lottie => "application/json",
            FileType::Other => "application/octet-stream",
        }
    }

    /// The file extension used when exporting.
    #[must_use]
    pub const fn ext(self) -> &'static str {
        match self {
            FileType::Png | FileType::Apng => "png",
            FileType::Jpeg => "jpg",
            FileType::Gif => "gif",
            FileType::Webp => "webp",
            FileType::Avif => "avif",
            FileType::Mp3 => "mp3",
            FileType::Ogg => "ogg",
            FileType::Lottie => "json",
            FileType::Other => "bin",
        }
    }
}

pub const KIB: u64 = 1024;
pub const MIB: u64 = 1024 * 1024;

/// One kind's limits, with the wording used in errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: u64,
    pub types: &'static [FileType],
    /// Longest sound, in milliseconds.
    pub max_ms: Option<u64>,
    /// e.g. `256 KiB, JPEG/PNG/GIF/WebP/AVIF`.
    pub label: &'static str,
}

/// The whole message's attachments together.
pub const ATTACHMENTS_PER_REQUEST: u64 = 25 * MIB;

/// The limits for each kind (research E4).
#[must_use]
pub const fn limits(kind: AssetKind) -> Limits {
    use FileType as F;
    match kind {
        AssetKind::Emoji => Limits {
            max_bytes: 256 * KIB,
            types: &[F::Jpeg, F::Png, F::Apng, F::Gif, F::Webp, F::Avif],
            max_ms: None,
            label: "256 KiB, JPEG/PNG/GIF/WebP/AVIF",
        },
        AssetKind::Sticker => Limits {
            max_bytes: 512 * KIB,
            types: &[F::Png, F::Apng, F::Gif, F::Lottie],
            max_ms: None,
            label: "512 KiB, PNG/APNG/GIF",
        },
        AssetKind::Sound => Limits {
            max_bytes: 512_000,
            types: &[F::Mp3, F::Ogg],
            max_ms: Some(5_200),
            label: "MP3/Ogg, 512 KB, ≤ 5.2 s",
        },
        AssetKind::Attachment => Limits {
            max_bytes: 20 * MIB,
            types: &[F::Png, F::Apng, F::Jpeg, F::Gif, F::Webp, F::Avif, F::Mp3, F::Ogg, F::Lottie, F::Other],
            max_ms: None,
            label: "20 MiB per file, 25 MiB per request",
        },
        AssetKind::GuildImage | AssetKind::Avatar => Limits {
            max_bytes: 10 * MIB,
            types: &[F::Png, F::Apng, F::Jpeg, F::Gif, F::Webp],
            max_ms: None,
            label: "10 MiB, PNG/JPEG/GIF/WebP",
        },
    }
}

/// Why an asset can't be used. The message names the kind's limit.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetError {
    #[error("{kind} is {size} bytes, over the limit ({limit})")]
    TooLarge { kind: &'static str, size: u64, limit: &'static str },
    #[error("{kind} must be {limit}; this file is {found}")]
    WrongType { kind: &'static str, found: String, limit: &'static str },
    #[error("{kind} is {ms} ms long, over the limit ({limit})")]
    TooLong { kind: &'static str, ms: u64, limit: &'static str },
    #[error("{kind}: couldn't read the sound's length; use a standard MP3 or Ogg (Vorbis/Opus) file")]
    UnknownLength { kind: &'static str },
    #[error("attachments add up to {size} bytes, over 25 MiB per request")]
    RequestTooLarge { size: u64 },
    #[error("inline data isn't valid base64: {0}")]
    BadData(String),
    #[error("the file is empty")]
    Empty,
}

fn is_apng(bytes: &[u8]) -> bool {
    // An `acTL` chunk before the first `IDAT` marks an animated PNG.
    let mut i = 8;
    while i + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let kind = &bytes[i + 4..i + 8];
        if kind == b"acTL" {
            return true;
        }
        if kind == b"IDAT" {
            return false;
        }
        i = i.saturating_add(12).saturating_add(len);
    }
    false
}

fn looks_like_lottie(bytes: &[u8]) -> bool {
    let text = std::str::from_utf8(bytes).unwrap_or("");
    let t = text.trim_start();
    t.starts_with('{') && t.contains("\"layers\"") && t.contains("\"v\"")
}

/// The file type, from the bytes alone.
#[must_use]
pub fn detect(bytes: &[u8]) -> FileType {
    match infer::get(bytes).map(|t| t.mime_type()) {
        Some("image/png") if is_apng(bytes) => FileType::Apng,
        Some("image/png") => FileType::Png,
        Some("image/jpeg") => FileType::Jpeg,
        Some("image/gif") => FileType::Gif,
        Some("image/webp") => FileType::Webp,
        Some("image/avif") => FileType::Avif,
        Some("audio/mpeg") => FileType::Mp3,
        Some("audio/ogg" | "audio/opus") => FileType::Ogg,
        _ if looks_like_lottie(bytes) => FileType::Lottie,
        _ if mp3_frames_start(bytes).is_some() => FileType::Mp3,
        _ => FileType::Other,
    }
}

/// Checks bytes against a kind's limits before upload. Returns the detected type.
pub fn check(kind: AssetKind, bytes: &[u8]) -> Result<FileType, AssetError> {
    if bytes.is_empty() {
        return Err(AssetError::Empty);
    }
    let l = limits(kind);
    let found = detect(bytes);
    if !l.types.contains(&found) {
        let found = match found {
            FileType::Other => "not a recognised type".to_owned(),
            t => t.mime().to_owned(),
        };
        return Err(AssetError::WrongType { kind: kind.name(), found, limit: l.label });
    }
    let size = bytes.len() as u64;
    if size > l.max_bytes {
        return Err(AssetError::TooLarge { kind: kind.name(), size, limit: l.label });
    }
    if let Some(max) = l.max_ms {
        let ms = duration_ms(found, bytes).ok_or(AssetError::UnknownLength { kind: kind.name() })?;
        if ms > max {
            return Err(AssetError::TooLong { kind: kind.name(), ms, limit: l.label });
        }
    }
    Ok(found)
}

/// Checks the total size of a message's attachments.
pub fn check_request(total_bytes: u64) -> Result<(), AssetError> {
    if total_bytes > ATTACHMENTS_PER_REQUEST { Err(AssetError::RequestTooLarge { size: total_bytes }) } else { Ok(()) }
}

/// blake3 of the bytes, as hex: how plans compare a local asset with the live one.
#[must_use]
pub fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// A `data:` URI for uploading through JSON (emoji, sounds, server images, avatars).
#[must_use]
pub fn data_uri(t: FileType, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", t.mime(), STANDARD.encode(bytes))
}

/// Decodes inline data: a `data:…;base64,` URI or bare base64.
pub fn decode_data(data: &str) -> Result<Vec<u8>, AssetError> {
    let b64 = match data.trim().strip_prefix("data:") {
        Some(rest) => rest
            .split_once(";base64,")
            .map(|(_, b)| b)
            .ok_or_else(|| AssetError::BadData("a data URI must be `data:<type>;base64,<data>`".to_owned()))?,
        None => data.trim(),
    };
    let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD.decode(clean).map_err(|e| AssetError::BadData(e.to_string()))
}

// ---- sound length ---------------------------------------------------------------------------

/// A sound's length in milliseconds, for MP3 and Ogg (Vorbis or Opus).
#[must_use]
pub fn duration_ms(t: FileType, bytes: &[u8]) -> Option<u64> {
    match t {
        FileType::Mp3 => mp3_duration_ms(bytes),
        FileType::Ogg => ogg_duration_ms(bytes),
        _ => None,
    }
}

/// Where MPEG audio frames start, after an `ID3v2` tag if there is one.
fn mp3_frames_start(bytes: &[u8]) -> Option<usize> {
    let start = if bytes.len() >= 10 && &bytes[..3] == b"ID3" {
        let size = bytes[6..10].iter().fold(0usize, |acc, b| (acc << 7) | usize::from(b & 0x7f));
        10 + size
    } else {
        0
    };
    mp3_frame(bytes.get(start..)?).map(|_| start)
}

/// Bitrates in kbps by index: MPEG-1 Layer III, and MPEG-2/2.5 Layer III.
const MP3_V1_KBPS: [usize; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
const MP3_V2_KBPS: [usize; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

/// One MPEG audio frame header: (frame length in bytes, samples, sample rate).
fn mp3_frame(b: &[u8]) -> Option<(usize, u64, u64)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (b[1] >> 3) & 0b11; // 3 = MPEG1, 2 = MPEG2, 0 = MPEG2.5
    let layer = (b[1] >> 1) & 0b11; // 1 = Layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let bitrate_idx = usize::from(b[2] >> 4);
    let rate_idx = usize::from((b[2] >> 2) & 0b11);
    let padding = usize::from((b[2] >> 1) & 1);
    if bitrate_idx == 0 || bitrate_idx == 15 || rate_idx == 3 {
        return None;
    }
    let (kbps, rate, samples, coef) = match version {
        3 => (MP3_V1_KBPS[bitrate_idx], [44_100, 48_000, 32_000][rate_idx], 1152, 144),
        2 => (MP3_V2_KBPS[bitrate_idx], [22_050, 24_000, 16_000][rate_idx], 576, 72),
        _ => (MP3_V2_KBPS[bitrate_idx], [11_025, 12_000, 8_000][rate_idx], 576, 72),
    };
    let len = coef * kbps * 1000 / rate + padding;
    Some((len, samples, rate as u64))
}

fn mp3_duration_ms(bytes: &[u8]) -> Option<u64> {
    let mut i = mp3_frames_start(bytes)?;
    let (mut samples, mut rate) = (0u64, 0u64);
    while let Some((len, s, r)) = bytes.get(i..).and_then(mp3_frame) {
        samples += s;
        rate = r;
        i += len.max(1);
    }
    (rate > 0).then(|| samples * 1000 / rate)
}

fn ogg_duration_ms(bytes: &[u8]) -> Option<u64> {
    // The identification header in the first page gives the codec and sample rate; the last
    // page's granule position is the total sample count.
    let first = bytes.windows(4).position(|w| w == b"OggS")?;
    let body = bytes.get(first + 27 + usize::from(*bytes.get(first + 26)?)..)?;
    let rate: u64 = if body.starts_with(b"OpusHead") {
        48_000
    } else if body.len() >= 16 && &body[1..7] == b"vorbis" {
        u64::from(u32::from_le_bytes([body[12], body[13], body[14], body[15]]))
    } else {
        return None;
    };
    let pre_skip = if body.starts_with(b"OpusHead") && body.len() >= 12 {
        u64::from(u16::from_le_bytes([body[10], body[11]]))
    } else {
        0
    };
    let last = bytes.windows(4).rposition(|w| w == b"OggS")?;
    let g = bytes.get(last + 6..last + 14)?;
    let granule = u64::from_le_bytes([g[0], g[1], g[2], g[3], g[4], g[5], g[6], g[7]]);
    (rate > 0).then(|| granule.saturating_sub(pre_skip) * 1000 / rate)
}
