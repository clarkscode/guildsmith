//! Loading asset bytes (research E7): local files, URLs and inline data.
//!
//! Local files are read only inside allowed directories. Over MCP that is
//! `GUILDSMITH_FILE_ROOTS` (empty by default, so only URLs and inline data work); the CLI also
//! allows the spec file's own directory. Paths are canonicalized first, so `..` and symlinks
//! can't step outside a root.

use std::path::{Path, PathBuf};

use guildsmith_core::asset::{self, Asset, AssetKind, FileType};

use crate::error::ServiceError;
use crate::policy::{Policy, Refusal, RefusalKind};
use crate::ports::AssetPort;

/// Who is asking, which decides the allowed directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller<'a> {
    /// An MCP tool. `base` is the directory relative paths start from (a spec file's
    /// directory), if any; it must itself be under a root to be read.
    Mcp { base: Option<&'a Path> },
    /// The CLI with a spec file: its directory is allowed too.
    Cli { spec_dir: Option<&'a Path> },
}

/// Bytes read for an asset, checked against its kind's limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub bytes: Vec<u8>,
    pub file_type: FileType,
    /// blake3, as hex.
    pub hash: String,
}

/// The directories local files may come from.
#[must_use]
pub fn allowed_roots(policy: &Policy, caller: Caller<'_>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = policy.file_roots.clone();
    if let Caller::Cli { spec_dir: Some(d) } = caller {
        roots.push(d.to_path_buf());
    }
    roots.iter().filter_map(|r| r.canonicalize().ok()).collect()
}

fn outside(file: &str, roots: &[PathBuf]) -> ServiceError {
    let reason = if roots.is_empty() {
        format!("can't read `{file}`: local files are off (GUILDSMITH_FILE_ROOTS is empty)")
    } else {
        format!("can't read `{file}`: it is outside the allowed directories (GUILDSMITH_FILE_ROOTS)")
    };
    Refusal::new(
        RefusalKind::NotAllowed,
        reason,
        Some("add the directory to GUILDSMITH_FILE_ROOTS, or pass the asset as a url or inline data"),
    )
    .into()
}

/// The canonical path of `file` if it is inside an allowed root.
pub fn resolve_file(file: &str, base: Option<&Path>, roots: &[PathBuf]) -> Result<PathBuf, ServiceError> {
    let p = Path::new(file);
    let candidates: Vec<PathBuf> = if p.is_absolute() {
        vec![p.to_path_buf()]
    } else if let Some(b) = base {
        vec![b.join(p)]
    } else {
        roots.iter().map(|r| r.join(p)).collect()
    };
    if roots.is_empty() {
        return Err(outside(file, roots));
    }
    for c in candidates {
        if let Ok(real) = c.canonicalize() {
            return if roots.iter().any(|r| real.starts_with(r)) { Ok(real) } else { Err(outside(file, roots)) };
        }
    }
    Err(ServiceError::Io(format!("can't find `{file}`")))
}

/// A directory an MCP tool may write files into: under an allowed root (it may not exist yet).
pub fn writable_dir(policy: &Policy, dir: &str) -> Result<PathBuf, ServiceError> {
    let roots = allowed_roots(policy, Caller::Mcp { base: None });
    let p = Path::new(dir);
    // The nearest part of the path that exists, canonicalized, decides where it really is.
    let mut existing = p;
    while !existing.exists() {
        existing = existing.parent().filter(|x| !x.as_os_str().is_empty()).unwrap_or(Path::new("."));
    }
    let real = existing.canonicalize().map_err(|e| ServiceError::Io(format!("can't use `{dir}`: {e}")))?;
    if roots.is_empty() || !roots.iter().any(|r| real.starts_with(r)) {
        return Err(outside(dir, &roots));
    }
    Ok(real.join(p.strip_prefix(existing).unwrap_or(Path::new(""))))
}

/// Reads an asset's raw bytes, at most `max_bytes`.
pub async fn load_bytes<P: AssetPort>(
    port: &P,
    policy: &Policy,
    caller: Caller<'_>,
    a: &Asset,
    max_bytes: u64,
) -> Result<Vec<u8>, ServiceError> {
    let too_big = |size: u64| ServiceError::Invalid(format!("{a} is {size} bytes, over the {max_bytes}-byte limit"));
    read(port, policy, caller, a, max_bytes, too_big).await
}

async fn read<P: AssetPort>(
    port: &P,
    policy: &Policy,
    caller: Caller<'_>,
    a: &Asset,
    max_bytes: u64,
    too_big: impl Fn(u64) -> ServiceError,
) -> Result<Vec<u8>, ServiceError> {
    match a {
        Asset::Data { data } => {
            let bytes = asset::decode_data(data).map_err(|e| ServiceError::Invalid(e.to_string()))?;
            if bytes.len() as u64 > max_bytes {
                return Err(too_big(bytes.len() as u64));
            }
            Ok(bytes)
        }
        Asset::Url { url } => {
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err(ServiceError::Invalid(format!("`{url}` isn't an http(s) URL")));
            }
            // One byte over the limit, so an oversized body is reported with its kind's limit.
            Ok(port.fetch_url(url, max_bytes + 1).await?)
        }
        Asset::File { file } => {
            let roots = allowed_roots(policy, caller);
            let base = match caller {
                Caller::Mcp { base } => base,
                Caller::Cli { spec_dir } => spec_dir,
            };
            let path = resolve_file(file, base, &roots)?;
            let meta = std::fs::metadata(&path).map_err(|e| ServiceError::Io(format!("can't read `{file}`: {e}")))?;
            if meta.len() > max_bytes {
                return Err(too_big(meta.len()));
            }
            std::fs::read(&path).map_err(|e| ServiceError::Io(format!("can't read `{file}`: {e}")))
        }
    }
}

/// Reads an asset and checks it for `kind`: type, size and (for sounds) length.
pub async fn load<P: AssetPort>(
    port: &P,
    policy: &Policy,
    caller: Caller<'_>,
    a: &Asset,
    kind: AssetKind,
) -> Result<Loaded, ServiceError> {
    let l = asset::limits(kind);
    let too_big = |size: u64| {
        ServiceError::Invalid(asset::AssetError::TooLarge { kind: kind.name(), size, limit: l.label }.to_string())
    };
    let bytes = read(port, policy, caller, a, l.max_bytes, too_big).await?;
    let file_type = asset::check(kind, &bytes).map_err(|e| ServiceError::Invalid(e.to_string()))?;
    Ok(Loaded { hash: asset::hash(&bytes), bytes, file_type })
}
