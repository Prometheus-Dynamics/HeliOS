use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

/// Wrapper over `Arc<T>` used for handler injection.
#[derive(Clone)]
pub struct AsyncApiData<T>(pub Arc<T>);

impl<T> AsyncApiData<T> {
    /// Create a new `AsyncApiData` from an [`Arc`].
    pub fn new(data: Arc<T>) -> Self {
        Self(data)
    }

    /// Access the wrapped [`Arc`].
    pub fn into_inner(self) -> Arc<T> {
        self.0
    }
}

impl<T> std::ops::Deref for AsyncApiData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Generic path extractor for command handlers.
#[derive(Clone, Debug)]
pub struct AsyncApiPath<T>(pub T);

impl<T> AsyncApiPath<T> {
    /// Create a new `AsyncApiPath` from a value.
    pub fn new(val: T) -> Self {
        Self(val)
    }

    /// Consume the wrapper and return the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> std::ops::Deref for AsyncApiPath<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// JSON payload extractor for command handlers.
#[derive(Clone, Debug)]
pub struct AsyncApiPayload<T>(pub T);

impl<T> AsyncApiPayload<T> {
    /// Create a new `AsyncApiPayload` from a value.
    pub fn new(val: T) -> Self {
        Self(val)
    }

    /// Consume the wrapper and return the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> std::ops::Deref for AsyncApiPayload<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Simple type-based map for storing shared data.
#[derive(Clone)]
pub struct AsyncApiDataMap(HashMap<TypeId, Arc<dyn Any + Send + Sync>>);

impl AsyncApiDataMap {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert<T: Any + Send + Sync>(&mut self, val: Arc<T>) {
        self.0.insert(TypeId::of::<T>(), val);
    }

    pub fn get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.0.get(&TypeId::of::<T>()).and_then(|v| v.clone().downcast::<T>().ok())
    }
}

impl Default for AsyncApiDataMap {
    fn default() -> Self {
        Self::new()
    }
}

pub trait FromSegments: Sized {
    fn from_segments(segs: &[&str]) -> Option<Self>;
}

impl FromSegments for () {
    fn from_segments(_segs: &[&str]) -> Option<Self> {
        Some(())
    }
}

impl<A: FromStr> FromSegments for (A,) {
    fn from_segments(segs: &[&str]) -> Option<Self> {
        if segs.len() != 1 {
            return None;
        }
        Some((segs[0].parse().ok()?,))
    }
}

impl<A: FromStr, B: FromStr> FromSegments for (A, B) {
    fn from_segments(segs: &[&str]) -> Option<Self> {
        if segs.len() != 2 {
            return None;
        }
        Some((segs[0].parse().ok()?, segs[1].parse().ok()?))
    }
}

impl<A: FromStr, B: FromStr, C: FromStr> FromSegments for (A, B, C) {
    fn from_segments(segs: &[&str]) -> Option<Self> {
        if segs.len() != 3 {
            return None;
        }
        Some((segs[0].parse().ok()?, segs[1].parse().ok()?, segs[2].parse().ok()?))
    }
}

impl<A: FromStr, B: FromStr, C: FromStr, D: FromStr> FromSegments for (A, B, C, D) {
    fn from_segments(segs: &[&str]) -> Option<Self> {
        if segs.len() != 4 {
            return None;
        }
        Some((segs[0].parse().ok()?, segs[1].parse().ok()?, segs[2].parse().ok()?, segs[3].parse().ok()?))
    }
}

impl<T: FromSegments> AsyncApiPath<T> {
    pub fn extract(pattern: &str, path: &str) -> Option<Self> {
        let pat_parts: Vec<&str> = pattern.split('.').collect();
        let path_parts: Vec<&str> = path.split('.').collect();
        if pat_parts.len() != path_parts.len() {
            return None;
        }
        let mut caps = Vec::new();
        for (p, v) in pat_parts.iter().zip(path_parts.iter()) {
            if p.starts_with('{') && p.ends_with('}') {
                caps.push(*v);
            } else if p != v {
                return None;
            }
        }
        T::from_segments(&caps).map(AsyncApiPath)
    }
}
