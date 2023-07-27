// Copyright (c) 2023 MASSA LABS <info@massa.net>
use crate::versioning::{ComponentStateTypeId, MipComponent, MipInfo};
use massa_proto_rs::massa::model::v1 as grpc_model;

impl From<&ComponentStateTypeId> for grpc_model::ComponentStateId {
    fn from(value: &ComponentStateTypeId) -> Self {
        match value {
            ComponentStateTypeId::Error => grpc_model::ComponentStateId::Error,
            ComponentStateTypeId::Defined => grpc_model::ComponentStateId::Defined,
            ComponentStateTypeId::Started => grpc_model::ComponentStateId::Started,
            ComponentStateTypeId::LockedIn => grpc_model::ComponentStateId::Lockedin,
            ComponentStateTypeId::Active => grpc_model::ComponentStateId::Active,
            ComponentStateTypeId::Failed => grpc_model::ComponentStateId::Failed,
        }
    }
}

impl From<&MipComponent> for grpc_model::MipComponent {
    fn from(value: &MipComponent) -> Self {
        match value {
            MipComponent::KeyPair => grpc_model::MipComponent::Keypair,
            MipComponent::Address => grpc_model::MipComponent::Address,
            _ => grpc_model::MipComponent::Unspecified,
        }
    }
}

impl From<&MipInfo> for grpc_model::MipInfo {
    fn from(value: &MipInfo) -> Self {
        let components = value
            .components
            .iter()
            .map(|(mip_component, version)| grpc_model::MipComponentEntry {
                kind: grpc_model::MipComponent::from(mip_component).into(),
                version: *version,
            })
            .collect();
        Self {
            name: value.name.clone(),
            version: value.version,
            start: Some(value.start.into()),
            timeout: Some(value.timeout.into()),
            activation_delay: Some(value.activation_delay.into()),
            components,
        }
    }
}
