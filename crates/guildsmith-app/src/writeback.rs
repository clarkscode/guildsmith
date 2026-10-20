//! Writing ids back into the user's spec file (research E9). Only `id = "…"` (or `code = "…"`
//! for invites) lines are added or changed, through `toml_edit`, so comments and formatting
//! everywhere else stay byte for byte.

use std::path::Path;

use guildsmith_core::spec::PathSeg;

use crate::error::ServiceError;
use crate::services::apply::WrittenId;

/// The table at `path` (e.g. `emoji[0]`), if the document has it.
fn table<'d>(doc: &'d mut toml_edit::DocumentMut, path: &[PathSeg]) -> Option<&'d mut toml_edit::Table> {
    let mut item: &mut toml_edit::Item = doc.as_item_mut();
    let mut segs = path.iter().peekable();
    while let Some(seg) = segs.next() {
        item = match seg {
            PathSeg::Key(k) => item.get_mut(k.as_str())?,
            PathSeg::Index(i) => match item {
                toml_edit::Item::ArrayOfTables(a) => {
                    let t = a.get_mut(*i)?;
                    if segs.peek().is_none() {
                        return Some(t);
                    }
                    // Deeper paths aren't written back.
                    return None;
                }
                _ => return None,
            },
        };
    }
    item.as_table_mut()
}

/// `text` with each id set. Returns the new text and how many lines changed.
pub fn apply_ids(text: &str, ids: &[WrittenId]) -> Result<(String, usize), ServiceError> {
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|e| ServiceError::Invalid(format!("the spec file can't be edited: {e}")))?;
    let mut changed = 0;
    for w in ids {
        let Some(t) = table(&mut doc, &w.spec_path.0) else {
            tracing::warn!(path = %w.path, "no table to write the id into");
            continue;
        };
        if t.get(&w.field).and_then(toml_edit::Item::as_str) == Some(w.value.as_str()) {
            continue;
        }
        if let Some(v) = t.get_mut(&w.field).and_then(toml_edit::Item::as_value_mut) {
            // Keep the line's own spacing and comment, changing only the value.
            let decor = v.decor().clone();
            *v = toml_edit::Value::from(w.value.as_str());
            *v.decor_mut() = decor;
        } else {
            t.insert(&w.field, toml_edit::value(w.value.as_str()));
            // New keys go first, where ids are written by export.
            let later = |k: &toml_edit::Key| !matches!(k.get(), "id" | "code");
            t.sort_values_by(|a, _, b, _| later(a).cmp(&later(b)));
        }
        changed += 1;
    }
    Ok((doc.to_string(), changed))
}

/// Writes the ids into the file at `path`. Returns how many lines changed.
pub fn write_ids(path: &Path, ids: &[WrittenId]) -> Result<usize, ServiceError> {
    if ids.is_empty() {
        return Ok(0);
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| ServiceError::Io(format!("can't read {}: {e}", path.display())))?;
    let (new, changed) = apply_ids(&text, ids)?;
    if changed > 0 {
        std::fs::write(path, new).map_err(|e| ServiceError::Io(format!("can't write {}: {e}", path.display())))?;
    }
    Ok(changed)
}
