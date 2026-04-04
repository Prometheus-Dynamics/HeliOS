use super::super::config::session_idle_timeout;
use super::support::env_lock;

#[test]
fn session_idle_timeout_uses_namespaced_policy() {
    let _lock = env_lock();
    unsafe { std::env::set_var("HELIOS_PERIPHERALS_SESSION_IDLE_MS", "1500") };
    assert_eq!(session_idle_timeout(), std::time::Duration::from_millis(1500));
    unsafe { std::env::remove_var("HELIOS_PERIPHERALS_SESSION_IDLE_MS") };
}
