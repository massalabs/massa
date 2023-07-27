// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::MassaTime;
use massa_proto_rs::massa::model::v1 as grpc_model;

impl From<MassaTime> for grpc_model::NativeTime {
    fn from(value: MassaTime) -> Self {
        grpc_model::NativeTime {
            milliseconds: value.to_millis(),
        }
    }
}
