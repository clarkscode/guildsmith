//! T009: asset type detection and the limits per kind (research E4): emoji "256 KiB,
//! JPEG/PNG/GIF/WebP/AVIF"; sticker "512 KiB, PNG/APNG/GIF"; sound "MP3/Ogg, 512 KB, ≤ 5.2 s";
//! message attachment "20 MiB per file, 25 MiB per request".

use guildsmith_core::asset::{self, AssetError, AssetKind, FileType, KIB, MIB};

fn png(size: usize) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    // IHDR chunk, then IDAT.
    b.extend([0, 0, 0, 13]);
    b.extend(b"IHDR");
    b.extend([0; 13 + 4]);
    b.extend([0, 0, 0, 0]);
    b.extend(b"IDAT");
    b.resize(size.max(b.len()), 0);
    b
}

fn apng() -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    b.extend([0, 0, 0, 13]);
    b.extend(b"IHDR");
    b.extend([0; 13 + 4]);
    b.extend([0, 0, 0, 8]);
    b.extend(b"acTL");
    b.extend([0; 8 + 4]);
    b.extend([0, 0, 0, 0]);
    b.extend(b"IDAT");
    b
}

fn jpeg(size: usize) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0];
    b.resize(size, 0);
    b
}

fn gif() -> Vec<u8> {
    let mut b = b"GIF89a".to_vec();
    b.resize(64, 0);
    b
}

fn webp() -> Vec<u8> {
    let mut b = b"RIFF\x24\x00\x00\x00WEBPVP8 ".to_vec();
    b.resize(64, 0);
    b
}

fn avif() -> Vec<u8> {
    let mut b = vec![0, 0, 0, 0x1C];
    b.extend(b"ftypavif");
    b.extend([0, 0, 0, 0]);
    b.extend(b"avifmif1miaf");
    b.resize(64, 0);
    b
}

/// MPEG-1 Layer III frames at 128 kbps / 44.1 kHz: 417 bytes and 1152 samples each.
fn mp3(frames: usize) -> Vec<u8> {
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x64];
    frame.resize(417, 0);
    frame.repeat(frames)
}

fn ogg_page(granule: u64, body: &[u8]) -> Vec<u8> {
    let mut p = b"OggS".to_vec();
    p.push(0); // version
    p.push(0); // header type
    p.extend(granule.to_le_bytes());
    p.extend(1u32.to_le_bytes()); // serial
    p.extend(0u32.to_le_bytes()); // sequence
    p.extend(0u32.to_le_bytes()); // crc (not checked)
    p.push(1);
    p.push(u8::try_from(body.len()).unwrap());
    p.extend(body);
    p
}

/// An Opus stream of `secs` seconds (48 kHz, no pre-skip).
fn ogg_opus(secs: u64) -> Vec<u8> {
    let mut head = b"OpusHead".to_vec();
    head.extend([1, 2, 0, 0]);
    head.extend(48_000u32.to_le_bytes());
    head.extend([0, 0, 0]);
    let mut out = ogg_page(0, &head);
    out.extend(ogg_page(48_000 * secs, &[0; 32]));
    out
}

#[test]
fn types_are_detected_from_bytes() {
    assert_eq!(asset::detect(&png(100)), FileType::Png);
    assert_eq!(asset::detect(&apng()), FileType::Apng);
    assert_eq!(asset::detect(&jpeg(100)), FileType::Jpeg);
    assert_eq!(asset::detect(&gif()), FileType::Gif);
    assert_eq!(asset::detect(&webp()), FileType::Webp);
    assert_eq!(asset::detect(&avif()), FileType::Avif);
    assert_eq!(asset::detect(&mp3(3)), FileType::Mp3);
    assert_eq!(asset::detect(&ogg_opus(1)), FileType::Ogg);
    assert_eq!(asset::detect(br#"{"v":"5.7.4","layers":[]}"#), FileType::Lottie);
    assert_eq!(asset::detect(b"plain text"), FileType::Other);
}

#[test]
fn emoji_limit_is_256_kib_and_image_types() {
    assert_eq!(asset::check(AssetKind::Emoji, &png(1000)), Ok(FileType::Png));
    for ok in [jpeg(100), gif(), webp(), avif()] {
        assert!(asset::check(AssetKind::Emoji, &ok).is_ok());
    }
    let big = asset::check(AssetKind::Emoji, &png(usize::try_from(256 * KIB).unwrap() + 1)).unwrap_err();
    assert!(matches!(big, AssetError::TooLarge { .. }));
    assert!(big.to_string().contains("256 KiB, JPEG/PNG/GIF/WebP/AVIF"), "{big}");
    assert!(asset::check(AssetKind::Emoji, &png(usize::try_from(256 * KIB).unwrap())).is_ok());
    let wrong = asset::check(AssetKind::Emoji, &mp3(2)).unwrap_err();
    assert!(wrong.to_string().contains("256 KiB, JPEG/PNG/GIF/WebP/AVIF"), "{wrong}");
}

#[test]
fn sticker_limit_is_512_kib_png_apng_gif() {
    assert_eq!(asset::check(AssetKind::Sticker, &apng()), Ok(FileType::Apng));
    assert!(asset::check(AssetKind::Sticker, &gif()).is_ok());
    let wrong = asset::check(AssetKind::Sticker, &jpeg(100)).unwrap_err();
    assert!(wrong.to_string().contains("512 KiB, PNG/APNG/GIF"), "{wrong}");
    let big = asset::check(AssetKind::Sticker, &png(usize::try_from(512 * KIB).unwrap() + 1)).unwrap_err();
    assert!(big.to_string().contains("512 KiB, PNG/APNG/GIF"), "{big}");
}

#[test]
fn sound_limit_is_mp3_or_ogg_512_kb_and_5_2_seconds() {
    // 191 frames ≈ 4.99 s.
    assert_eq!(asset::duration_ms(FileType::Mp3, &mp3(191)), Some(4989));
    assert_eq!(asset::check(AssetKind::Sound, &mp3(191)), Ok(FileType::Mp3));
    let long = asset::check(AssetKind::Sound, &mp3(250)).unwrap_err();
    assert!(matches!(long, AssetError::TooLong { .. }), "{long:?}");
    assert!(long.to_string().contains("MP3/Ogg, 512 KB, ≤ 5.2 s"), "{long}");

    assert_eq!(asset::duration_ms(FileType::Ogg, &ogg_opus(4)), Some(4000));
    assert!(asset::check(AssetKind::Sound, &ogg_opus(5)).is_ok());
    assert!(matches!(asset::check(AssetKind::Sound, &ogg_opus(6)), Err(AssetError::TooLong { .. })));

    // 512 KB is 512,000 bytes: a short but padded file over it is refused for size.
    let mut padded = mp3(10);
    padded.resize(512_001, 0);
    assert!(matches!(asset::check(AssetKind::Sound, &padded), Err(AssetError::TooLarge { .. })));
    assert!(matches!(asset::check(AssetKind::Sound, &png(100)), Err(AssetError::WrongType { .. })));
}

#[test]
fn attachments_are_20_mib_per_file_and_25_mib_per_request() {
    assert!(asset::check(AssetKind::Attachment, b"any text file").is_ok());
    let big = asset::check(AssetKind::Attachment, &jpeg(usize::try_from(20 * MIB).unwrap() + 1)).unwrap_err();
    assert!(big.to_string().contains("20 MiB per file, 25 MiB per request"), "{big}");
    assert!(asset::check_request(25 * MIB).is_ok());
    assert!(asset::check_request(25 * MIB + 1).is_err());
}

#[test]
fn hashing_and_data_uris() {
    let a = png(100);
    assert_eq!(asset::hash(&a), asset::hash(&a.clone()));
    assert_ne!(asset::hash(&a), asset::hash(&png(101)));
    let uri = asset::data_uri(FileType::Png, &a);
    assert!(uri.starts_with("data:image/png;base64,"));
    assert_eq!(asset::decode_data(&uri).unwrap(), a);
    assert_eq!(asset::decode_data(uri.trim_start_matches("data:image/png;base64,")).unwrap(), a);
    assert!(asset::decode_data("data:image/png,notbase64").is_err());
    assert!(matches!(asset::check(AssetKind::Emoji, &[]), Err(AssetError::Empty)));
}
