//! The robot facts HeliOS works with: the FRC team and the NetworkTables server robot code runs on.
//!
//! Each fact has one source, by precedence: the user's setting on this device, then what the robot
//! controller reports (the SystemCore HeliOS runs on, or the controller a board is plugged into over
//! USB), then nothing. helios-api resolves them for the UI and the recorder, and helios-engine for
//! publishing; both call [`resolve`], so they always agree. The roboRIO address formula
//! (`10.TE.AM.2`) lives here too, once.

mod resolve;
mod settings;

pub use resolve::{ControllerFacts, Effective, Fact, Source, resolve};
pub use settings::{DEFAULT_NT4_PORT, DEFAULT_NT4_PREFIX, Nt4Settings, TEAM_RANGE, roborio_address};
