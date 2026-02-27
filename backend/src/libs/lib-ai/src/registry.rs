#[cfg(feature = "docs")]
use crate::docs::{BackendDoc, DocumentedBackend};
use crate::{
    backend::{AiBackend, BackendCapabilities, BackendFeature, BackendKind},
    model::ModelFormat,
    tensor::TensorElementType,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub use self::error::{Error, Result};

mod error;

pub type ParamValue = serde_json::Value;
pub type ParamType = String;

type BackendCreateFn<B> = fn(Vec<(String, ParamValue)>) -> Result<Arc<B>>;
type BackendDefaultFn<B> = fn() -> Result<Arc<B>>;

pub trait Registerable<B>: Send + Sync
where
    B: AiBackend + ?Sized,
{
    fn backend_kind() -> BackendKind;
    fn hardware_accelerated() -> bool;
    fn supported_model_formats() -> Vec<ModelFormat>;
    fn supported_precisions() -> Vec<TensorElementType>;
    fn create(params: Vec<(String, ParamValue)>) -> Result<Arc<B>>;
    fn get_create_params() -> Vec<(String, ParamType)>;

    fn default() -> Result<Arc<B>> {
        Self::create(Vec::new())
    }

    fn features() -> Vec<BackendFeature> {
        Vec::new()
    }

    fn max_batch_size() -> Option<usize> {
        None
    }

    fn capabilities() -> BackendCapabilities {
        BackendCapabilities {
            kind: Self::backend_kind(),
            hardware_accelerated: Self::hardware_accelerated(),
            supported_model_formats: Self::supported_model_formats(),
            supported_precisions: Self::supported_precisions(),
            max_batch_size: Self::max_batch_size(),
            features: Self::features(),
        }
    }
}

struct BackendFactory<B>
where
    B: AiBackend + ?Sized,
{
    backend_kind: BackendKind,
    create_fn: BackendCreateFn<B>,
    default_fn: BackendDefaultFn<B>,
    get_create_params_fn: fn() -> Vec<(String, ParamType)>,
    capabilities: BackendCapabilities,
    #[cfg(feature = "docs")]
    docs_fn: fn() -> BackendDoc,
}

impl<B> BackendFactory<B>
where
    B: AiBackend + ?Sized,
{
    #[cfg(not(feature = "docs"))]
    fn new<R>() -> Self
    where
        R: Registerable<B> + 'static,
    {
        Self { backend_kind: R::backend_kind(), create_fn: R::create, default_fn: R::default, get_create_params_fn: R::get_create_params, capabilities: R::capabilities() }
    }

    #[cfg(feature = "docs")]
    fn new<R>() -> Self
    where
        R: Registerable<B> + DocumentedBackend + 'static,
    {
        Self { backend_kind: R::backend_kind(), create_fn: R::create, default_fn: R::default, get_create_params_fn: R::get_create_params, capabilities: R::capabilities(), docs_fn: R::docs }
    }

    #[cfg(feature = "docs")]
    fn docs(&self) -> BackendDoc {
        (self.docs_fn)()
    }

    fn create(&self, params: Vec<(String, ParamValue)>) -> Result<Arc<B>> {
        (self.create_fn)(params)
    }

    fn default(&self) -> Result<Arc<B>> {
        (self.default_fn)()
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.capabilities.clone()
    }
}

pub struct Registry<B>
where
    B: AiBackend + ?Sized,
{
    backends: RwLock<HashMap<String, BackendFactory<B>>>,
}

impl<B> Default for Registry<B>
where
    B: AiBackend + ?Sized,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<B> Registry<B>
where
    B: AiBackend + ?Sized,
{
    pub fn new() -> Self {
        Registry { backends: RwLock::new(HashMap::new()) }
    }

    #[cfg(not(feature = "docs"))]
    pub fn register_backend<R>(&self, name: &str)
    where
        R: Registerable<B> + 'static,
    {
        let factory = BackendFactory::<B>::new::<R>();
        let mut backends = self.backends.write().expect("registry write lock poisoned");
        backends.insert(name.to_string(), factory);
    }

    #[cfg(feature = "docs")]
    pub fn register_backend<R>(&self, name: &str)
    where
        R: Registerable<B> + DocumentedBackend + 'static,
    {
        let factory = BackendFactory::<B>::new::<R>();
        let mut backends = self.backends.write().expect("registry write lock poisoned");
        backends.insert(name.to_string(), factory);
    }

    pub fn list_backends(&self) -> Vec<BackendInfo> {
        let backends = self.backends.read().expect("registry read lock poisoned");
        backends
            .iter()
            .map(|(name, factory)| BackendInfo {
                name: name.clone(),
                backend_kind: factory.backend_kind.clone(),
                create_params: (factory.get_create_params_fn)(),
                capabilities: factory.capabilities(),
                #[cfg(feature = "docs")]
                docs: factory.docs(),
            })
            .collect()
    }

    pub fn create_backend(&self, name: &str, params: Vec<(String, ParamValue)>) -> Result<Arc<B>> {
        let backends = self.backends.read().expect("registry read lock poisoned");
        if let Some(factory) = backends.get(name) { factory.create(params) } else { Err(Error::BackendNotFound { name: name.to_string() }) }
    }

    pub fn create_backend_default(&self, name: &str) -> Result<Arc<B>> {
        let backends = self.backends.read().expect("registry read lock poisoned");
        if let Some(factory) = backends.get(name) { factory.default() } else { Err(Error::BackendNotFound { name: name.to_string() }) }
    }

    pub fn get_backend(&self, name: &str) -> Result<BackendInfo> {
        let backends = self.backends.read().expect("registry read lock poisoned");
        if let Some(factory) = backends.get(name) {
            Ok(BackendInfo {
                name: name.to_string(),
                backend_kind: factory.backend_kind.clone(),
                create_params: (factory.get_create_params_fn)(),
                capabilities: factory.capabilities(),
                #[cfg(feature = "docs")]
                docs: factory.docs(),
            })
        } else {
            Err(Error::BackendNotFound { name: name.to_string() })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfo {
    pub name: String,
    pub backend_kind: BackendKind,
    pub create_params: Vec<(String, ParamType)>,
    pub capabilities: BackendCapabilities,
    #[cfg(feature = "docs")]
    pub docs: BackendDoc,
}
