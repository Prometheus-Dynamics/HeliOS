use lib_asyncapi::{AsyncApiDataMap, AsyncApiDoc, AsyncApiInfo, ExternalDocs, SchemaRegistry, Server, Tag, WsDoc};
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;
use url::Url;

#[derive(Clone)]
pub struct TransportMetadata {
    info: AsyncApiInfo,
    id: Option<Url>,
    tags: Vec<Tag>,
    external_docs: Option<ExternalDocs>,
    servers: BTreeMap<String, Server>,
}

impl TransportMetadata {
    pub fn new() -> Self {
        Self { info: AsyncApiInfo::default(), id: None, tags: Vec::new(), external_docs: None, servers: BTreeMap::new() }
    }

    pub fn info(&self) -> &AsyncApiInfo {
        &self.info
    }

    pub fn info_mut(&mut self) -> &mut AsyncApiInfo {
        &mut self.info
    }

    pub fn set_info(&mut self, info: AsyncApiInfo) {
        self.info = info;
    }

    pub fn id(&self) -> Option<&Url> {
        self.id.as_ref()
    }

    pub fn set_id(&mut self, id: Url) {
        self.id = Some(id);
    }

    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    pub fn tags_mut(&mut self) -> &mut Vec<Tag> {
        &mut self.tags
    }

    pub fn add_tag(&mut self, tag: Tag) {
        self.tags.push(tag);
    }

    pub fn external_docs(&self) -> Option<&ExternalDocs> {
        self.external_docs.as_ref()
    }

    pub fn set_external_docs(&mut self, docs: ExternalDocs) {
        self.external_docs = Some(docs);
    }

    pub fn servers(&self) -> &BTreeMap<String, Server> {
        &self.servers
    }

    pub fn servers_mut(&mut self) -> &mut BTreeMap<String, Server> {
        &mut self.servers
    }

    pub fn add_server(&mut self, name: impl Into<String>, server: Server) {
        self.servers.insert(name.into(), server);
    }
}

impl Default for TransportMetadata {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct TransportContext {
    data: AsyncApiDataMap,
    schemas: SchemaRegistry,
    metadata: TransportMetadata,
}

impl TransportContext {
    pub fn new() -> Self {
        Self { data: AsyncApiDataMap::new(), schemas: SchemaRegistry::new(), metadata: TransportMetadata::new() }
    }

    pub fn add_data<T: Any + Send + Sync>(&mut self, data: Arc<T>) {
        self.data.insert(data);
    }

    pub fn get_data<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.data.get::<T>()
    }

    pub fn metadata(&self) -> &TransportMetadata {
        &self.metadata
    }

    pub fn metadata_mut(&mut self) -> &mut TransportMetadata {
        &mut self.metadata
    }

    pub fn schemas(&self) -> &SchemaRegistry {
        &self.schemas
    }

    pub fn schemas_mut(&mut self) -> &mut SchemaRegistry {
        &mut self.schemas
    }

    pub fn asyncapi_doc(&self, docs: &[WsDoc], loop_docs: &[WsDoc]) -> AsyncApiDoc {
        lib_asyncapi::generate_asyncapi(
            docs,
            loop_docs,
            &self.schemas,
            self.metadata.info.clone(),
            self.metadata.servers.clone(),
            self.metadata.id.clone(),
            &self.metadata.tags,
            self.metadata.external_docs.clone(),
        )
    }

    pub fn asyncapi_json(&self, docs: &[WsDoc], loop_docs: &[WsDoc]) -> serde_json::Value {
        serde_json::to_value(self.asyncapi_doc(docs, loop_docs)).expect("serialize asyncapi")
    }
}

impl Default for TransportContext {
    fn default() -> Self {
        Self::new()
    }
}
