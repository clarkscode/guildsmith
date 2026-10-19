//! `cargo xtask sync-plugins`: copies the skills in `plugins/skills/` (the single source,
//! research R15) into each plugin. Stale files in the copies are removed. `check-plugins` fails CI
//! when a copy drifts.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// The source of every skill.
pub(crate) const SOURCE: &str = "plugins/skills";

/// Where each plugin keeps its copy.
pub(crate) const COPIES: &[&str] = &["plugins/claude-code/skills", "plugins/codex/skills"];

/// Every file under `dir`, keyed by its `/`-separated path relative to `dir`.
pub(crate) fn files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
        for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                walk(base, &path, out)?;
            } else {
                let rel = path.strip_prefix(base).expect("under base");
                let key = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
                out.insert(key, std::fs::read(&path).with_context(|| format!("read {}", path.display()))?);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    if dir.exists() {
        walk(dir, dir, &mut out)?;
    }
    Ok(out)
}

pub fn run(_args: &[String]) -> Result<()> {
    let root = crate::root();
    let source = files(&root.join(SOURCE))?;
    if source.is_empty() {
        bail!("{SOURCE}/ has no skills to copy");
    }
    for copy in COPIES {
        let dir = root.join(copy);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
        }
        for (rel, bytes) in &source {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("file has a parent"))?;
            std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
        }
        println!("synced {} file(s) into {copy}/", source.len());
    }
    Ok(())
}
