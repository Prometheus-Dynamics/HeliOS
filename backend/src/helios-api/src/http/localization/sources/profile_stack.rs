use std::sync::Arc;

#[derive(Clone, Default)]
pub(in crate::http::localization) struct ProfileResolveStack {
    entries: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ProfileResolveStack {
    pub(in crate::http::localization) fn enter(&self, profile_id: &str) -> Result<ProfileResolveGuard, String> {
        let mut stack = self.entries.lock().expect("profile resolve stack mutex poisoned");
        if stack.iter().any(|entry| entry == profile_id) {
            return Err(format!("profile source cycle detected for '{profile_id}'"));
        }
        stack.push(profile_id.to_string());
        Ok(ProfileResolveGuard { entries: Arc::clone(&self.entries), profile_id: profile_id.to_string() })
    }

    #[cfg(test)]
    fn snapshot(&self) -> Vec<String> {
        self.entries.lock().expect("profile resolve stack mutex poisoned").clone()
    }
}

#[derive(Debug)]
pub(in crate::http::localization) struct ProfileResolveGuard {
    entries: Arc<std::sync::Mutex<Vec<String>>>,
    profile_id: String,
}

impl Drop for ProfileResolveGuard {
    fn drop(&mut self) {
        let mut stack = self.entries.lock().expect("profile resolve stack mutex poisoned");
        if let Some(index) = stack.iter().rposition(|entry| entry == &self.profile_id) {
            stack.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProfileResolveStack;

    #[test]
    fn profile_resolve_stack_releases_entries_after_repeated_sampling() {
        let stack = ProfileResolveStack::default();

        for _ in 0..16 {
            let guard = stack.enter("profile-a").expect("first profile entry");
            assert_eq!(stack.snapshot(), vec!["profile-a".to_string()]);
            drop(guard);
            assert!(stack.snapshot().is_empty());
        }
    }

    #[test]
    fn profile_resolve_stack_detects_cycles_without_leaking_entries() {
        let stack = ProfileResolveStack::default();
        let outer = stack.enter("outer").expect("outer entry");
        let inner = stack.enter("inner").expect("inner entry");

        let err = stack.enter("outer").expect_err("cycle to be rejected");
        assert!(err.contains("cycle detected"));
        assert_eq!(stack.snapshot(), vec!["outer".to_string(), "inner".to_string()]);

        drop(inner);
        drop(outer);
        assert!(stack.snapshot().is_empty());

        let guard = stack.enter("outer").expect("stack to be reusable after cleanup");
        drop(guard);
        assert!(stack.snapshot().is_empty());
    }
}
