//! Settings the device keeps for its UI, which used to live in the browser: the names, icons and
//! colours of cameras and other objects (`identities.json`), whether first-time setup is done
//! (`setup.json`), and the robot controller's facts HeliOS found (`controller.json`, see
//! `host_facts`). Each one is a store document, so every write is announced like the others.

use std::collections::BTreeMap;

use helios_robot::ControllerFacts;
use serde::{Deserialize, Serialize};

use crate::{
    error::{ApiError, ApiResult},
    store::{Store, read_json},
};

/// The colours an identity can take: an index into the theme's palette (`--id-1` .. `--id-12`).
pub const COLORS: std::ops::RangeInclusive<u8> = 1..=12;

/// A user-chosen name, icon and colour for an object (a camera, a pipeline, a screen).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub color: u8,
}

impl Identity {
    pub fn validate(&self) -> ApiResult<()> {
        if !COLORS.contains(&self.color) {
            return Err(ApiError::unprocessable("color is 1 to 12"));
        }
        if self.name.as_deref().is_some_and(|name| name.chars().count() > 120) {
            return Err(ApiError::unprocessable("name is at most 120 characters"));
        }
        if self.icon.as_deref().is_some_and(|icon| icon.len() > 40 || !icon.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) {
            return Err(ApiError::unprocessable("icon is a short icon name (letters, digits and -)"));
        }
        Ok(())
    }
}

/// First-time setup: done once the user finishes (or skips) the wizard.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SetupRecord {
    pub onboarded: bool,
}

impl Store {
    fn identities_path(&self) -> std::path::PathBuf {
        self.dir.join("identities.json")
    }

    fn setup_path(&self) -> std::path::PathBuf {
        self.dir.join("setup.json")
    }

    fn controller_path(&self) -> std::path::PathBuf {
        self.dir.join("controller.json")
    }

    /// Every stored identity, by object id.
    pub async fn identities(&self) -> ApiResult<BTreeMap<String, Identity>> {
        let _guard = self.lock.lock().await;
        read_json(&self.identities_path()).await.map(Option::unwrap_or_default)
    }

    /// Keep `identity` for `id`; returns every stored identity.
    pub async fn put_identity(&self, id: &str, identity: Identity) -> ApiResult<BTreeMap<String, Identity>> {
        let _guard = self.lock.lock().await;
        let path = self.identities_path();
        let mut all: BTreeMap<String, Identity> = read_json(&path).await?.unwrap_or_default();
        all.insert(id.to_string(), identity);
        self.save("identities", id, &path, &all).await?;
        Ok(all)
    }

    /// Forget the identity of `id` (its defaults apply again); false when there was none.
    pub async fn delete_identity(&self, id: &str) -> ApiResult<bool> {
        let _guard = self.lock.lock().await;
        let path = self.identities_path();
        let mut all: BTreeMap<String, Identity> = read_json(&path).await?.unwrap_or_default();
        if all.remove(id).is_none() {
            return Ok(false);
        }
        self.save("identities", id, &path, &all).await?;
        Ok(true)
    }

    pub async fn setup(&self) -> ApiResult<SetupRecord> {
        let _guard = self.lock.lock().await;
        read_json(&self.setup_path()).await.map(Option::unwrap_or_default)
    }

    pub async fn put_setup(&self, record: &SetupRecord) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        self.save("setup", "onboarding", &self.setup_path(), record).await
    }

    /// The robot controller HeliOS found, if any.
    pub async fn controller_facts(&self) -> ApiResult<Option<ControllerFacts>> {
        let _guard = self.lock.lock().await;
        read_json(&self.controller_path()).await
    }

    /// Keep what was found about the robot controller; `None` forgets it (no controller reachable).
    pub async fn put_controller_facts(&self, facts: Option<&ControllerFacts>) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        let path = self.controller_path();
        match facts {
            Some(facts) => self.save("robot", "controller", &path, facts).await,
            None => match tokio::fs::remove_file(&path).await {
                Ok(()) => {
                    self.notify_changed("robot", "controller");
                    Ok(())
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{ApiEvent, EventHub};
    use std::sync::Arc;

    fn facts(team: Option<u32>) -> ControllerFacts {
        ControllerFacts { kind: "systemcore".into(), name: "SystemCore".into(), team, nt4_server: Some("127.0.0.1".into()), via: "local".into(), seen_at_ms: 1 }
    }

    /// The next `settings` event on `receiver`, as (domain, key).
    async fn next_setting(receiver: &mut tokio::sync::broadcast::Receiver<ApiEvent>) -> (String, String) {
        loop {
            let event = receiver.recv().await.expect("event");
            if event.kind == "settings" {
                return (event.data["domain"].as_str().unwrap().into(), event.data["key"].as_str().unwrap().into());
            }
        }
    }

    #[tokio::test]
    async fn every_write_announces_its_domain_and_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let hub = Arc::new(EventHub::new(64));
        let store = Store::new(dir.path()).with_events(hub.clone());
        let mut events = hub.subscribe();

        store.put_identity("camera.a", Identity { name: Some("Front".into()), icon: None, color: 3 }).await.expect("identity");
        assert_eq!(next_setting(&mut events).await, ("identities".into(), "camera.a".into()));
        store.put_setup(&SetupRecord { onboarded: true }).await.expect("setup");
        assert_eq!(next_setting(&mut events).await, ("setup".into(), "onboarding".into()));
        store.put_controller_facts(Some(&facts(Some(1234)))).await.expect("facts");
        assert_eq!(next_setting(&mut events).await, ("robot".into(), "controller".into()));
        store.put_controller_facts(None).await.expect("forget facts");
        assert_eq!(next_setting(&mut events).await, ("robot".into(), "controller".into()));
        assert!(store.delete_identity("camera.a").await.expect("delete"));
        assert_eq!(next_setting(&mut events).await, ("identities".into(), "camera.a".into()));
        store.put_nt4_settings(&helios_robot::Nt4Settings { team: Some(1234), ..Default::default() }).await.expect("nt4");
        assert_eq!(next_setting(&mut events).await, ("nt4".into(), "settings".into()));
    }

    #[tokio::test]
    async fn writes_without_a_hub_are_kept_and_nothing_is_announced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        store.put_setup(&SetupRecord { onboarded: true }).await.expect("setup");
        let store = Store::new(dir.path());
        assert!(store.setup().await.expect("read").onboarded);
    }

    #[tokio::test]
    async fn identities_and_controller_facts_survive_a_new_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        store.put_identity("tags-front", Identity { name: None, icon: Some("target".into()), color: 8 }).await.expect("put");
        store.put_controller_facts(Some(&facts(Some(6390)))).await.expect("facts");
        let store = Store::new(dir.path());
        assert_eq!(store.identities().await.expect("read")["tags-front"].color, 8);
        assert_eq!(store.controller_facts().await.expect("facts").and_then(|f| f.team), Some(6390));
    }

    #[test]
    fn identity_fields_are_checked() {
        assert!(Identity { name: None, icon: None, color: 12 }.validate().is_ok());
        assert!(Identity { name: None, icon: None, color: 0 }.validate().is_err());
        assert!(Identity { name: None, icon: Some("../etc".into()), color: 1 }.validate().is_err());
        assert!(Identity { name: Some("x".repeat(121)), icon: None, color: 1 }.validate().is_err());
    }
}
