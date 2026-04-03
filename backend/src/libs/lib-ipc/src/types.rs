use std::{collections::BTreeSet, fmt, str::FromStr};

use chrono::{DateTime, Utc};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type Timestamp = DateTime<Utc>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(transparent)]
pub struct CommandId(Uuid);

impl CommandId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    #[must_use]
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for CommandId {
    fn from(uuid: Uuid) -> Self {
        Self::from_uuid(uuid)
    }
}

impl From<CommandId> for Uuid {
    fn from(value: CommandId) -> Self {
        value.0
    }
}

impl Default for CommandId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    pub const fn is_compatible(&self, other: &Self) -> bool {
        self.major == other.major
    }
}

impl Default for ProtocolVersion {
    fn default() -> Self {
        Self::new(1, 0)
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseProtocolVersionError {
    input: String,
}

impl ParseProtocolVersionError {
    fn new(input: impl Into<String>) -> Self {
        Self { input: input.into() }
    }
}

impl fmt::Display for ParseProtocolVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid protocol version: {}", self.input)
    }
}

impl std::error::Error for ParseProtocolVersionError {}

impl FromStr for ProtocolVersion {
    type Err = ParseProtocolVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split('.').collect();
        if parts.len() != 2 {
            return Err(ParseProtocolVersionError::new(s));
        }

        let major: u16 = parts[0].parse().map_err(|_| ParseProtocolVersionError::new(s))?;
        let minor: u16 = parts[1].parse().map_err(|_| ParseProtocolVersionError::new(s))?;

        Ok(Self::new(major, minor))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct FeatureSet {
    features: BTreeSet<String>,
}

impl FeatureSet {
    #[must_use]
    pub fn new<I, S>(features: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let features = features.into_iter().map(Into::into).collect();
        Self { features }
    }

    #[must_use]
    pub fn contains(&self, feature: &str) -> bool {
        self.features.contains(feature)
    }

    pub fn insert(&mut self, feature: impl Into<String>) {
        self.features.insert(feature.into());
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.features.iter()
    }

    #[must_use]
    pub fn intersection(&self, other: &Self) -> Self {
        let mut accepted = Self::default();
        for feature in self.iter() {
            if other.contains(feature.as_str()) {
                accepted.insert(feature.clone());
            }
        }
        accepted
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct JournalMetadata {
    pub file_name: String,
    #[rkyv(with = crate::archive::with::SerdeBytes)]
    pub created_at: Timestamp,
    #[rkyv(with = crate::archive::with::SerdeBytes)]
    pub last_trimmed_at: Option<Timestamp>,
    pub entries: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_from_str_parses_major_minor() {
        let version: ProtocolVersion = "2.5".parse().expect("parse");
        assert_eq!(version.major, 2);
        assert_eq!(version.minor, 5);
    }

    #[test]
    fn protocol_version_from_str_rejects_invalid_input() {
        assert!("abc".parse::<ProtocolVersion>().is_err());
        assert!("1".parse::<ProtocolVersion>().is_err());
        assert!("1.2.3".parse::<ProtocolVersion>().is_err());
    }
}
