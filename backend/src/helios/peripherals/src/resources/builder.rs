use crate::model::{NodeId, ResourceDescriptor, ResourceId, ResourceKind, ResourceStatus};

pub struct ResourceBuilder {
    resource: ResourceDescriptor,
}

impl ResourceBuilder {
    pub fn new(owner: NodeId, kind: ResourceKind, local: impl AsRef<str>, display_name: impl Into<String>) -> Result<Self, orion::core::OrionError> {
        Ok(Self { resource: ResourceDescriptor::from_parts(owner, kind, local, display_name)? })
    }

    pub fn status(mut self, status: ResourceStatus) -> Self {
        self.resource.status = status;
        self
    }

    pub fn capability(mut self, name: impl Into<String>, detail: Option<impl Into<String>>) -> Self {
        self.resource.add_capability(name.into(), detail.map(|d| d.into()));
        self
    }

    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.resource.set_label(key.into(), value.into());
        self
    }

    pub fn endpoint(mut self, protocol: impl Into<String>, address: impl Into<String>) -> Self {
        self.resource.add_endpoint(protocol.into(), address.into());
        self
    }

    pub fn link(mut self, target: ResourceId, relation: impl Into<String>) -> Self {
        self.resource.add_link(target, relation.into());
        self
    }

    pub fn build(self) -> ResourceDescriptor {
        self.resource
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_core_resource_fields() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner.clone(), ResourceKind::CaptureDevice, "cam0", "Camera 0")
            .expect("resource")
            .capability("capture", Some("styx"))
            .label("capture_role", "camera_stream")
            .endpoint("v4l2", "/dev/video0")
            .build();

        assert_eq!(resource.owner, owner);
        assert_eq!(resource.kind, ResourceKind::CaptureDevice);
        assert_eq!(resource.label("capture_role"), Some("camera_stream"));
        assert_eq!(resource.endpoint("v4l2"), Some("/dev/video0"));
        assert!(resource.capabilities.iter().any(|capability| capability.name.as_ref() == "capture"));
    }
}
