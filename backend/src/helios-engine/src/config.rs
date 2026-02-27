#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    pub name: String,
    /// Optional override for the Daedalus executor pool size (parallel graph runs). If unset, Daedalus picks based on available cores.
    pub daedalus_pool_size: Option<usize>,
}
