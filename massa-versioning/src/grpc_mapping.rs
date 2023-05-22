// Copyright (c) 2023 MASSA LABS <info@massa.net>
use crate::versioning::{ComponentStateTypeId, MipInfo};
use massa_proto::massa::api::v1 as grpc;

impl From<&ComponentStateTypeId> for grpc::ComponentStateId {
    fn from(value: &ComponentStateTypeId) -> Self {
        match value {
            ComponentStateTypeId::Error => grpc::ComponentStateId::Error,
            ComponentStateTypeId::Defined => grpc::ComponentStateId::Defined,
            ComponentStateTypeId::Started => grpc::ComponentStateId::Started,
            ComponentStateTypeId::LockedIn => grpc::ComponentStateId::Lockedin,
            ComponentStateTypeId::Active => grpc::ComponentStateId::Active,
            ComponentStateTypeId::Failed => grpc::ComponentStateId::Failed,
        }
    }
}

impl From<&MipInfo> for grpc::MipInfo {
    fn from(value: &MipInfo) -> Self {
        let components = value
            .components
            .iter()
            .map(|(mip_component, version)| grpc::MipComponentEntry {
                kind: u32::from(mip_component.clone()),
                version: *version,
            })
            .collect();
        Self {
            name: value.name.clone(),
            version: value.version,
            start: value.start.to_millis(),
            timeout: value.start.to_millis(),
            activation_delay: value.activation_delay.to_millis(),
            components,
        }
    }
}
