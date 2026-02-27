use std::any::TypeId;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

type SchemaCallback = dyn Fn(&mut BTreeMap<String, serde_json::Value>) + Send + Sync + 'static;

#[derive(Clone)]
pub struct SchemaFn(Arc<SchemaCallback>);

impl SchemaFn {
    pub fn new<F>(registrar: F) -> Self
    where
        F: Fn(&mut BTreeMap<String, serde_json::Value>) + Send + Sync + 'static,
    {
        Self(Arc::new(registrar))
    }

    pub fn call(&self, map: &mut BTreeMap<String, serde_json::Value>) {
        (self.0)(map);
    }
}

#[derive(Clone, Default)]
pub struct SchemaRegistry {
    seen: HashSet<TypeId>,
    registrars: Vec<(TypeId, SchemaFn)>,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self { seen: HashSet::with_capacity(capacity), registrars: Vec::with_capacity(capacity) }
    }

    pub fn track<T, F>(&mut self, registrar: F)
    where
        T: 'static + ?Sized,
        F: Fn(&mut BTreeMap<String, serde_json::Value>) + Send + Sync + 'static,
    {
        let id = TypeId::of::<T>();
        if self.seen.insert(id) {
            self.registrars.push((id, SchemaFn::new(registrar)));
        }
    }

    pub fn collect(&self) -> BTreeMap<String, serde_json::Value> {
        let mut map = BTreeMap::new();
        self.apply(&mut map);
        map
    }

    pub fn apply(&self, map: &mut BTreeMap<String, serde_json::Value>) {
        for (_, registrar) in &self.registrars {
            registrar.call(map);
        }
    }
}
