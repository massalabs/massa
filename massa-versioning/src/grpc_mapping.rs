// Copyright (c) 2023 MASSA LABS <info@massa.net>
use crate::versioning::{ComponentStateTypeId, MipComponent, MipInfo};
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

impl From<&MipComponent> for grpc::MipComponent {
    fn from(value: &MipComponent) -> Self {
        match value {
            MipComponent::KeyPair => grpc::MipComponent::Keypair,
            MipComponent::Address => grpc::MipComponent::Address,
            _ => grpc::MipComponent::Unspecified,
        }
    }
}

impl From<&MipInfo> for grpc::MipInfo {
    fn from(value: &MipInfo) -> Self {
        let components = value
            .components
            .iter()
            .map(|(mip_component, version)| grpc::MipComponentEntry {
                kind: grpc::MipComponent::from(mip_component).into(),
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
