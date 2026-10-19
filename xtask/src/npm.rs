//! `cargo xtask npm-pack <artifacts-dir> [--out <dir>] [--allow-missing]`: stages the npm
//! packages (research R15, ADR-002) from dist's release archives.
//!
//! The templates in `npm/` are copied to `--out` (default `target/npm/`), every version is set to
//! the workspace version, and each platform package gets the binary from its dist archive in
//! `bin/`. The release workflow then runs `npm publish` in each staged directory, platform
//! packages first. Nothing is published from here.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// dist target triple → npm platform package directory (`@guildsmith/<dir>`).
const PLATFORMS: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-musl", "linux-x64"),
    ("aarch64-unknown-linux-musl", "linux-arm64"),
    ("x86_64-apple-darwin", "darwin-x64"),
    ("aarch64-apple-darwin", "darwin-arm64"),
    ("x86_64-pc-windows-msvc", "win32-x64"),
];

const ARCHIVE_EXTS: &[&str] = &["tar.xz", "tar.gz", "zip"];

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    std::fs::write(path, text).with_context(|| format!("write {}", path.display()))
}

fn read_json(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

/// Finds `guildsmith-<target>.<ext>` anywhere under `dir`.
fn find_archive(dir: &Path, target: &str) -> Result<Option<PathBuf>> {
    let names: Vec<String> = ARCHIVE_EXTS.iter().map(|e| format!("guildsmith-{target}.{e}")).collect();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            if let Some(found) = find_archive(&path, target)? {
                return Ok(Some(found));
            }
        } else if path.file_name().is_some_and(|n| names.iter().any(|x| n == x.as_str())) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Finds a file called `name` anywhere under `dir`.
fn find_file(dir: &Path, name: &str) -> Result<Option<PathBuf>> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name)? {
                return Ok(Some(found));
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn extract(archive: &Path, into: &Path) -> Result<()> {
    std::fs::create_dir_all(into)?;
    let is_zip = archive.extension().is_some_and(|e| e == "zip");
    let status = if is_zip && cfg!(not(windows)) {
        Command::new("unzip").arg("-q").arg("-o").arg(archive).arg("-d").arg(into).status()
    } else {
        // GNU tar reads .tar.xz/.tar.gz; Windows' bsdtar also reads .zip.
        Command::new("tar").arg("-xf").arg(archive).arg("-C").arg(into).status()
    }
    .with_context(|| format!("run the extractor for {}", archive.display()))?;
    if !status.success() {
        bail!("couldn't extract {}", archive.display());
    }
    Ok(())
}

fn copy_licenses(root: &Path, dir: &Path) -> Result<()> {
    for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
        std::fs::copy(root.join(name), dir.join(name)).with_context(|| format!("copy {name}"))?;
    }
    Ok(())
}

/// Stages one platform package with its binary. Returns `false` if the archive is missing.
fn stage_platform(root: &Path, artifacts: &Path, out: &Path, target: &str, dir: &str, version: &str) -> Result<bool> {
    let Some(archive) = find_archive(artifacts, target)? else {
        return Ok(false);
    };
    let pkg_dir = out.join(dir);
    let mut pkg = read_json(&root.join("npm/platforms").join(dir).join("package.json"))?;
    pkg["version"] = Value::from(version);
    std::fs::create_dir_all(pkg_dir.join("bin"))?;
    write_json(&pkg_dir.join("package.json"), &pkg)?;
    copy_licenses(root, &pkg_dir)?;

    let tmp = out.join(".extract").join(target);
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    extract(&archive, &tmp)?;
    let exe = if target.contains("windows") { "guildsmith.exe" } else { "guildsmith" };
    let bin = find_file(&tmp, exe)?.with_context(|| format!("{} has no {exe}", archive.display()))?;
    let dest = pkg_dir.join("bin").join(exe);
    std::fs::copy(&bin, &dest).with_context(|| format!("copy {exe} into {}", pkg_dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
    }
    println!("staged @guildsmith/{dir} from {}", archive.display());
    Ok(true)
}

pub fn run(args: &[String]) -> Result<()> {
    let usage = "usage: cargo xtask npm-pack <artifacts-dir> [--out <dir>] [--allow-missing]";
    let artifacts = PathBuf::from(args.first().filter(|a| !a.starts_with("--")).context(usage)?);
    if !artifacts.is_dir() {
        bail!("{} is not a directory\n{usage}", artifacts.display());
    }
    let root = crate::root();
    let out = crate::flag(args, "--out").map_or_else(|| root.join("target/npm"), PathBuf::from);
    let allow_missing = args.iter().any(|a| a == "--allow-missing");
    let version = crate::workspace_version()?;
    if out.exists() {
        std::fs::remove_dir_all(&out).with_context(|| format!("clear {}", out.display()))?;
    }

    let mut missing = Vec::new();
    for (target, dir) in PLATFORMS {
        if !stage_platform(&root, &artifacts, &out, target, dir, &version)? {
            missing.push(*target);
        }
    }
    let _ = std::fs::remove_dir_all(out.join(".extract"));
    if !missing.is_empty() && !allow_missing {
        bail!("no dist archive for: {} (looked under {})", missing.join(", "), artifacts.display());
    }

    let main_dir = out.join("guildsmith");
    std::fs::create_dir_all(main_dir.join("bin"))?;
    let mut pkg = read_json(&root.join("npm/guildsmith/package.json"))?;
    pkg["version"] = Value::from(version.as_str());
    if let Some(deps) = pkg["optionalDependencies"].as_object_mut() {
        for v in deps.values_mut() {
            *v = Value::from(version.as_str());
        }
    }
    write_json(&main_dir.join("package.json"), &pkg)?;
    std::fs::copy(root.join("npm/guildsmith/bin/guildsmith.js"), main_dir.join("bin/guildsmith.js"))?;
    std::fs::copy(root.join("README.md"), main_dir.join("README.md"))?;
    copy_licenses(&root, &main_dir)?;

    println!(
        "staged guildsmith@{version} and {} platform package(s) in {}",
        PLATFORMS.len() - missing.len(),
        out.display()
    );
    if !missing.is_empty() {
        println!("skipped (no archive): {}", missing.join(", "));
    }
    println!("publish the platform packages first, then guildsmith");
    Ok(())
}
