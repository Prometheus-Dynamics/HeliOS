use crate::{BoundedU64Policy, PathPolicy};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpcJournalPolicy {
    pub dir: PathPolicy,
    pub max_bytes: BoundedU64Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedIpcJournalPolicy {
    pub dir: PathBuf,
    pub max_bytes: u64,
}

impl IpcJournalPolicy {
    pub fn resolve(self) -> ResolvedIpcJournalPolicy {
        ResolvedIpcJournalPolicy { dir: self.dir.resolve(), max_bytes: self.max_bytes.resolve() }
    }
}

pub const HELIOS_IPC_JOURNAL_POLICY: IpcJournalPolicy = IpcJournalPolicy {
    dir: PathPolicy { env_var: "HELIOS_IPC_JOURNAL_DIR", default: "/var/lib/helios/journal/ipc" },
    max_bytes: BoundedU64Policy { env_var: "HELIOS_IPC_JOURNAL_MAX_BYTES", default: 8 * 1024 * 1024, min: 64 * 1024, max: 256 * 1024 * 1024 },
};
