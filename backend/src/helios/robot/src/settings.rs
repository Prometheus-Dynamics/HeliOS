//! The user's NetworkTables settings on this device (`nt4.json`).

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

pub const DEFAULT_NT4_PORT: u16 = 5810;
pub const DEFAULT_NT4_PREFIX: &str = "/helios";

/// The FRC team numbers that have a roboRIO address.
pub const TEAM_RANGE: RangeInclusive<u32> = 1..=25_599;

/// `10.TE.AM.2`, the roboRIO address WPILib uses for a team; `None` for a number outside [`TEAM_RANGE`].
pub fn roborio_address(team: u32) -> Option<String> {
    TEAM_RANGE.contains(&team).then(|| format!("10.{}.{}.2", team / 100, team % 100))
}

/// What the user set on this device. Every field is optional: the empty settings leave the
/// robot's facts to the robot controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Nt4Settings {
    /// Off: nothing is published, whatever the robot says.
    pub enabled: bool,
    /// FRC team number: the server is that team's roboRIO (`10.TE.AM.2`).
    pub team: Option<u32>,
    /// A server host instead of the team's roboRIO.
    pub server: Option<String>,
    pub port: u16,
    /// Topics are `<prefix>/<board>/<pipeline>/<output>`.
    pub prefix: String,
    /// This board's name in its topics; empty is its hostname. Boards on one robot share a server,
    /// and the board is what keeps their topics apart.
    pub board: String,
}

impl Default for Nt4Settings {
    fn default() -> Self {
        Self { enabled: true, team: None, server: None, port: DEFAULT_NT4_PORT, prefix: DEFAULT_NT4_PREFIX.into(), board: String::new() }
    }
}

impl Nt4Settings {
    /// The topic prefix with one leading slash and no trailing one (`/helios` when empty).
    pub fn topic_prefix(&self) -> String {
        let prefix = self.prefix.trim().trim_end_matches('/');
        if prefix.is_empty() {
            DEFAULT_NT4_PREFIX.into()
        } else if prefix.starts_with('/') {
            prefix.into()
        } else {
            format!("/{prefix}")
        }
    }

    /// Where this board's topics start: `<prefix>/<board>`.
    pub fn topic_base(&self) -> String {
        let board = self.board.trim();
        if board.is_empty() {
            let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
            format!("{}/{}", self.topic_prefix(), hostname.trim())
        } else {
            format!("{}/{board}", self.topic_prefix())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roborio_address_follows_the_team_number() {
        assert_eq!(roborio_address(6390).as_deref(), Some("10.63.90.2"));
        assert_eq!(roborio_address(254).as_deref(), Some("10.2.54.2"));
        assert_eq!(roborio_address(25_599).as_deref(), Some("10.255.99.2"));
        assert_eq!(roborio_address(0), None);
        assert_eq!(roborio_address(25_600), None);
    }

    #[test]
    fn boards_have_their_own_topics() {
        let named = |board: &str| Nt4Settings { board: board.into(), ..Nt4Settings::default() };
        assert_eq!(named("front").topic_base(), "/helios/front");
        assert_ne!(named("front").topic_base(), named("rear").topic_base());
        let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
        assert_eq!(named(" ").topic_base(), format!("/helios/{}", hostname.trim()));
    }

    #[test]
    fn topic_prefix_is_normalised() {
        assert_eq!(Nt4Settings::default().topic_prefix(), "/helios");
        assert_eq!(Nt4Settings { prefix: "helios/".into(), ..Default::default() }.topic_prefix(), "/helios");
        assert_eq!(Nt4Settings { prefix: " / ".into(), ..Default::default() }.topic_prefix(), "/helios");
        assert_eq!(Nt4Settings { prefix: "/vision".into(), ..Default::default() }.topic_prefix(), "/vision");
    }
}
