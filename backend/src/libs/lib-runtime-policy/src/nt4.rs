use crate::filesystem::PathPolicy;

pub const HELIOS_NT4_SETTINGS_FILE_POLICY: PathPolicy = PathPolicy { env_var: "HELIOS_NT4_SETTINGS_FILE", default: "/var/lib/helios/nt4.json" };

pub const HELIOS_TEAM_FILE_POLICY: PathPolicy = PathPolicy { env_var: "HELIOS_TEAM_FILE", default: "/var/lib/helios/team" };
