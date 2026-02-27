pub mod bridge;
pub mod limelight;
pub mod limelight_control;
pub mod limelight_types;
pub mod photonvision;
pub mod photonvision_packet;
pub mod pool;

use once_cell::sync::Lazy;

static NT4_POOL: Lazy<pool::Nt4ClientPool> = Lazy::new(pool::Nt4ClientPool::new);

pub fn pool() -> &'static pool::Nt4ClientPool {
    &NT4_POOL
}
