//! Discord snowflake ids.

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A Discord snowflake: stored as `u64`, serialized as a decimal string (Discord's JSON form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub u64);

impl Id {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Error for strings that are not a valid snowflake.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a Discord id (expected digits only)")]
pub struct ParseIdError(pub String);

impl FromStr for Id {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ParseIdError(s.to_owned()));
        }
        s.parse::<u64>().map(Id).map_err(|_| ParseIdError(s.to_owned()))
    }
}

impl From<u64> for Id {
    fn from(v: u64) -> Self {
        Id(v)
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Accept the string form (canonical) and plain integers (TOML/JSON written by hand).
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Str(String),
            Num(u64),
        }
        match Raw::deserialize(d)? {
            Raw::Str(s) => s.parse().map_err(serde::de::Error::custom),
            Raw::Num(n) => Ok(Id(n)),
        }
    }
}

impl JsonSchema for Id {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Id".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[0-9]{1,20}$",
            "description": "A Discord id (snowflake) as a decimal string."
        })
    }
}
