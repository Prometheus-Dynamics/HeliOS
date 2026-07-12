#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorLease {
    pub leader: String,
    pub expires_at_ms: u64,
}
