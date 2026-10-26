//! Resolving each robot fact by precedence: the user's setting, then the controller's, then none.

use serde::{Deserialize, Serialize};

use crate::settings::{Nt4Settings, TEAM_RANGE, roborio_address};

/// What a robot controller reports about itself, when HeliOS can reach it (`controller.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerFacts {
    /// The controller's kind (`systemcore`) and its display name.
    pub kind: String,
    pub name: String,
    /// The team the controller has, when it has one.
    pub team: Option<u32>,
    /// Where robot code's NetworkTables server answers: the controller's own address.
    pub nt4_server: Option<String>,
    /// How HeliOS reached it: `local` (HeliOS runs on it) or `usb` (its peer on the board's USB link).
    pub via: String,
    /// When it was first seen (ms since the epoch).
    pub seen_at_ms: u64,
}

impl ControllerFacts {
    /// The same controller, ignoring when it was first seen.
    pub fn same_controller(&self, other: &ControllerFacts) -> bool {
        ControllerFacts { seen_at_ms: 0, ..self.clone() } == ControllerFacts { seen_at_ms: 0, ..other.clone() }
    }
}

/// Where a fact came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    User,
    Controller,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fact<T> {
    pub value: Option<T>,
    pub source: Source,
}

impl<T> Fact<T> {
    fn by_precedence(user: Option<T>, controller: Option<T>) -> Self {
        match (user, controller) {
            (Some(value), _) => Self { value: Some(value), source: Source::User },
            (None, Some(value)) => Self { value: Some(value), source: Source::Controller },
            (None, None) => Self { value: None, source: Source::None },
        }
    }
}

/// The robot facts this device resolves to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Effective {
    pub team: Fact<u32>,
    /// The NetworkTables server publishing goes to (`None` when publishing is off or nothing is known).
    pub nt4_server: Fact<String>,
}

/// The effective facts: each is the user's setting when there is one, else the controller's, else none.
///
/// The server is the user's explicit host, else the user's team's roboRIO, else the controller's
/// NetworkTables server. Publishing switched off by the user (`enabled: false`) has no server, and
/// the controller does not change that.
pub fn resolve(settings: &Nt4Settings, controller: Option<&ControllerFacts>) -> Effective {
    let controller_team = controller.and_then(|facts| facts.team).filter(|team| TEAM_RANGE.contains(team));
    let team = Fact::by_precedence(settings.team.filter(|team| TEAM_RANGE.contains(team)), controller_team);
    if !settings.enabled {
        return Effective { team, nt4_server: Fact { value: None, source: Source::User } };
    }
    let explicit = settings.server.as_deref().map(str::trim).filter(|server| !server.is_empty()).map(str::to_string);
    let user_server = explicit.or_else(|| settings.team.and_then(roborio_address));
    let controller_server = controller.and_then(|facts| facts.nt4_server.as_deref()).map(str::trim).filter(|server| !server.is_empty()).map(str::to_string);
    Effective { team, nt4_server: Fact::by_precedence(user_server, controller_server) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn systemcore(team: Option<u32>, address: &str) -> ControllerFacts {
        ControllerFacts { kind: "systemcore".into(), name: "SystemCore".into(), team, nt4_server: Some(address.into()), via: "local".into(), seen_at_ms: 1 }
    }

    #[test]
    fn the_user_team_beats_the_controllers_team() {
        let controller = systemcore(Some(1234), "127.0.0.1");
        let user = Nt4Settings { team: Some(6390), ..Default::default() };
        let effective = resolve(&user, Some(&controller));
        assert_eq!((effective.team.value, effective.team.source), (Some(6390), Source::User));
        assert_eq!(effective.nt4_server.value.as_deref(), Some("10.63.90.2"), "the user's team gives the roboRIO");
        assert_eq!(effective.nt4_server.source, Source::User);
    }

    #[test]
    fn the_controller_provides_team_and_server_when_the_user_sets_neither() {
        let controller = systemcore(Some(1234), "127.0.0.1");
        let effective = resolve(&Nt4Settings::default(), Some(&controller));
        assert_eq!((effective.team.value, effective.team.source), (Some(1234), Source::Controller));
        assert_eq!((effective.nt4_server.value.as_deref(), effective.nt4_server.source), (Some("127.0.0.1"), Source::Controller));
    }

    #[test]
    fn an_explicit_server_beats_the_team_and_the_controller() {
        let controller = systemcore(Some(1234), "127.0.0.1");
        let user = Nt4Settings { team: Some(6390), server: Some(" 10.0.0.5 ".into()), ..Default::default() };
        let effective = resolve(&user, Some(&controller));
        assert_eq!(effective.nt4_server.value.as_deref(), Some("10.0.0.5"));
        assert_eq!(effective.nt4_server.source, Source::User);
    }

    #[test]
    fn nothing_known_resolves_to_none() {
        let effective = resolve(&Nt4Settings::default(), None);
        assert_eq!((effective.team.value, effective.team.source), (None, Source::None));
        assert_eq!((effective.nt4_server.value, effective.nt4_server.source), (None, Source::None));
    }

    #[test]
    fn a_controller_without_a_team_still_gives_its_server() {
        let effective = resolve(&Nt4Settings::default(), Some(&systemcore(None, "172.31.12.84")));
        assert_eq!((effective.team.value, effective.team.source), (None, Source::None));
        assert_eq!(effective.nt4_server.value.as_deref(), Some("172.31.12.84"));
        assert_eq!(effective.nt4_server.source, Source::Controller);
    }

    #[test]
    fn publishing_switched_off_has_no_server_whatever_the_controller_says() {
        let controller = systemcore(Some(1234), "127.0.0.1");
        let user = Nt4Settings { enabled: false, ..Default::default() };
        let effective = resolve(&user, Some(&controller));
        assert_eq!(effective.nt4_server.value, None);
        assert_eq!(effective.nt4_server.source, Source::User);
        assert_eq!(effective.team.source, Source::Controller, "the team is still reported");
    }

    #[test]
    fn out_of_range_teams_are_ignored() {
        let controller = systemcore(Some(0), "127.0.0.1");
        let user = Nt4Settings { team: Some(25_600), ..Default::default() };
        let effective = resolve(&user, Some(&controller));
        assert_eq!(effective.team.source, Source::None);
        assert_eq!(effective.nt4_server.source, Source::Controller);
    }

    #[test]
    fn same_controller_ignores_when_it_was_seen() {
        let first = systemcore(Some(1234), "127.0.0.1");
        let later = ControllerFacts { seen_at_ms: 99, ..first.clone() };
        assert!(first.same_controller(&later));
        assert!(!first.same_controller(&ControllerFacts { team: Some(1), ..first.clone() }));
    }
}
