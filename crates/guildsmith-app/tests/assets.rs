//! T010: asset loading. Over MCP, files outside `GUILDSMITH_FILE_ROOTS` are refused, and every
//! path is refused when it's empty. The CLI also allows the spec file's own directory. URLs
//! and inline data work without roots.

use std::path::{Path, PathBuf};

use guildsmith_app::assets::{self, Caller};
use guildsmith_app::error::ServiceError;
use guildsmith_app::fake::fixtures;
use guildsmith_app::policy::{Policy, RefusalKind};
use guildsmith_core::asset::{Asset, AssetKind, FileType};

fn png() -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    b.extend(b"IDAT");
    b.resize(200, 7);
    b
}

/// A fresh directory tree: `root/inside/logo.png` and `outside/secret.png`.
fn tree(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("guildsmith-assets-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    std::fs::create_dir_all(root.join("inside")).unwrap();
    std::fs::create_dir_all(base.join("outside")).unwrap();
    std::fs::write(root.join("inside/logo.png"), png()).unwrap();
    std::fs::write(base.join("outside/secret.png"), png()).unwrap();
    (base, root)
}

fn file(p: &Path) -> Asset {
    Asset::File { file: p.to_string_lossy().into_owned() }
}

fn refused_not_allowed(r: Result<impl std::fmt::Debug, ServiceError>) {
    match r {
        Err(ServiceError::Refused(r)) => {
            assert_eq!(r.kind, RefusalKind::NotAllowed);
            assert!(r.reason.contains("GUILDSMITH_FILE_ROOTS"), "{}", r.reason);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn mcp_reads_files_only_under_the_roots() {
    let (base, root) = tree("mcp");
    let fake = fixtures::lab_fake();
    let policy = Policy { file_roots: vec![root.clone()], ..Policy::default() };
    let mcp = Caller::Mcp { base: None };

    let ok = assets::load(&fake, &policy, mcp, &file(&root.join("inside/logo.png")), AssetKind::Emoji).await.unwrap();
    assert_eq!(ok.file_type, FileType::Png);
    assert_eq!(ok.bytes, png());
    // Relative paths start at a root.
    let rel = Asset::File { file: "inside/logo.png".to_owned() };
    assert!(assets::load(&fake, &policy, mcp, &rel, AssetKind::Emoji).await.is_ok());

    refused_not_allowed(
        assets::load(&fake, &policy, mcp, &file(&base.join("outside/secret.png")), AssetKind::Emoji).await,
    );
    // `..` can't step out of a root.
    let escape = Asset::File { file: "inside/../../outside/secret.png".to_owned() };
    refused_not_allowed(assets::load(&fake, &policy, mcp, &escape, AssetKind::Emoji).await);
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn with_no_roots_every_path_is_refused_over_mcp() {
    let (base, root) = tree("empty");
    let fake = fixtures::lab_fake();
    let policy = Policy::default();
    let mcp = Caller::Mcp { base: None };
    refused_not_allowed(
        assets::load(&fake, &policy, mcp, &file(&root.join("inside/logo.png")), AssetKind::Emoji).await,
    );
    let rel = Asset::File { file: "logo.png".to_owned() };
    refused_not_allowed(assets::load(&fake, &policy, mcp, &rel, AssetKind::Emoji).await);
    // A spec directory doesn't open anything over MCP.
    let with_base = Caller::Mcp { base: Some(&root) };
    refused_not_allowed(
        assets::load(&fake, &policy, with_base, &Asset::File { file: "inside/logo.png".to_owned() }, AssetKind::Emoji)
            .await,
    );
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn the_cli_also_allows_the_spec_files_directory() {
    let (base, root) = tree("cli");
    let fake = fixtures::lab_fake();
    let policy = Policy::default();
    let cli = Caller::Cli { spec_dir: Some(&root) };
    let rel = Asset::File { file: "inside/logo.png".to_owned() };
    assert!(assets::load(&fake, &policy, cli, &rel, AssetKind::Emoji).await.is_ok());
    refused_not_allowed(
        assets::load(&fake, &policy, cli, &file(&base.join("outside/secret.png")), AssetKind::Emoji).await,
    );
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn urls_and_inline_data_need_no_roots_and_are_checked() {
    let fake = fixtures::lab_fake();
    let policy = Policy::default();
    let mcp = Caller::Mcp { base: None };
    fake.set_url("https://example.com/logo.png", png());
    let url = Asset::Url { url: "https://example.com/logo.png".to_owned() };
    assert_eq!(assets::load(&fake, &policy, mcp, &url, AssetKind::Emoji).await.unwrap().bytes, png());

    let data = Asset::Data { data: guildsmith_core::asset::data_uri(FileType::Png, &png()) };
    let loaded = assets::load(&fake, &policy, mcp, &data, AssetKind::Sticker).await.unwrap();
    assert_eq!(loaded.hash, guildsmith_core::asset::hash(&png()));

    // A PNG isn't a sound: refused before any upload, naming the limit.
    let err = assets::load(&fake, &policy, mcp, &data, AssetKind::Sound).await.unwrap_err();
    assert!(err.to_string().contains("MP3/Ogg, 512 KB, ≤ 5.2 s"), "{err}");
    let not_http = Asset::Url { url: "file:///etc/passwd".to_owned() };
    assert!(matches!(
        assets::load(&fake, &policy, mcp, &not_http, AssetKind::Emoji).await,
        Err(ServiceError::Invalid(_))
    ));
}
