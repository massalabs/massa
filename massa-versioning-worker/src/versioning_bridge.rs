// Copyright (c) 2023 MASSA LABS <info@massa.net>
use crate::versioning::MipInfo;
use massa_proto::massa::api::v1 as grpc;

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
