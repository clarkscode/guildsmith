//! Parsing spec text and mapping paths inside it back to `line:col`.

use std::fmt;

use super::GuildSpec;

/// One step in a path into the spec, e.g. `roles[2].name`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSeg {
    Key(String),
    Index(usize),
}

/// A path into the spec document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SpecPath(pub Vec<PathSeg>);

impl SpecPath {
    #[must_use]
    pub fn root() -> Self {
        SpecPath(Vec::new())
    }

    #[must_use]
    pub fn key(mut self, k: &str) -> Self {
        self.0.push(PathSeg::Key(k.to_owned()));
        self
    }

    #[must_use]
    pub fn index(mut self, i: usize) -> Self {
        self.0.push(PathSeg::Index(i));
        self
    }
}

impl fmt::Display for SpecPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, seg) in self.0.iter().enumerate() {
            match seg {
                PathSeg::Key(k) if i == 0 => f.write_str(k)?,
                PathSeg::Key(k) => write!(f, ".{k}")?,
                PathSeg::Index(n) => write!(f, "[{n}]")?,
            }
        }
        Ok(())
    }
}

/// A 1-based line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, schemars::JsonSchema)]
pub struct Location {
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Spec text kept next to its parsed document, so errors can point at a line and column.
pub struct SpecSource<'a> {
    text: &'a str,
    doc: Option<toml_edit::Document<&'a str>>,
}

impl<'a> SpecSource<'a> {
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        SpecSource { text, doc: toml_edit::Document::parse(text).ok() }
    }

    /// Converts a byte offset into a 1-based line and column.
    #[must_use]
    pub fn offset_to_location(&self, offset: usize) -> Location {
        let offset = offset.min(self.text.len());
        let before = &self.text[..offset];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        Location { line, col }
    }

    /// Finds where `path` is written. Falls back to the closest parent that exists.
    #[must_use]
    pub fn locate(&self, path: &SpecPath) -> Option<Location> {
        let doc = self.doc.as_ref()?;
        let mut best = None;
        let mut item: Option<ItemRef<'_>> = Some(ItemRef::Table(doc.as_table()));
        for seg in &path.0 {
            let Some(cur) = item else { break };
            item = cur.child(seg);
            if let Some(span) = item.as_ref().and_then(ItemRef::span) {
                best = Some(span.start);
            }
        }
        best.map(|o| self.offset_to_location(o))
    }
}

enum ItemRef<'d> {
    Item(&'d toml_edit::Item),
    Table(&'d toml_edit::Table),
    Value(&'d toml_edit::Value),
}

impl<'d> ItemRef<'d> {
    fn span(&self) -> Option<std::ops::Range<usize>> {
        match self {
            ItemRef::Item(i) => i.span(),
            ItemRef::Table(t) => t.span(),
            ItemRef::Value(v) => v.span(),
        }
    }

    fn child(&self, seg: &PathSeg) -> Option<ItemRef<'d>> {
        match (self, seg) {
            (ItemRef::Table(t), PathSeg::Key(k)) => t.get(k).map(ItemRef::Item),
            (ItemRef::Item(toml_edit::Item::Table(t)), PathSeg::Key(k)) => t.get(k).map(ItemRef::Item),
            (ItemRef::Item(toml_edit::Item::ArrayOfTables(a)), PathSeg::Index(i)) => a.get(*i).map(ItemRef::Table),
            (ItemRef::Item(toml_edit::Item::Value(v)), seg) => value_child(v, seg),
            (ItemRef::Value(v), seg) => value_child(v, seg),
            _ => None,
        }
    }
}

fn value_child<'d>(v: &'d toml_edit::Value, seg: &PathSeg) -> Option<ItemRef<'d>> {
    match (v, seg) {
        (toml_edit::Value::Array(a), PathSeg::Index(i)) => a.get(*i).map(ItemRef::Value),
        (toml_edit::Value::InlineTable(t), PathSeg::Key(k)) => t.get(k).map(ItemRef::Value),
        _ => None,
    }
}

/// A spec file that failed to parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}{message}", location.map(|l| format!("{l}: ")).unwrap_or_default())]
pub struct ParseError {
    pub message: String,
    pub location: Option<Location>,
}

/// Parses spec text. Errors carry the line and column TOML reports.
pub fn parse(text: &str) -> Result<GuildSpec, ParseError> {
    toml::from_str::<GuildSpec>(text).map_err(|e| {
        let location = e.span().map(|s| SpecSource::new(text).offset_to_location(s.start));
        ParseError { message: e.message().to_owned(), location }
    })
}
