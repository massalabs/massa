// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{AsyncMessage, AsyncMessageTrigger};
use massa_proto::massa::api::v1 as grpc;

impl From<AsyncMessage> for grpc::AsyncMessage {
    fn from(value: AsyncMessage) -> Self {
        grpc::AsyncMessage {
            emission_slot: Some(value.emission_slot.into()),
            emission_index: value.emission_index,
            sender: value.sender.to_string(),
            destination: value.destination.to_string(),
            handler: value.handler.to_string(),
            max_gas: value.max_gas,
            fee: value.fee.to_raw(),
            coins: value.coins.to_raw(),
            validity_start: Some(value.validity_start.into()),
            validity_end: Some(value.validity_start.into()),
            data: value.data,
            trigger: value.trigger.map(|trigger| trigger.into()),
            can_be_executed: value.can_be_executed,
            hash: value.hash.to_string(),
        }
    }
}

impl From<AsyncMessageTrigger> for grpc::AsyncMessageTrigger {
    fn from(value: AsyncMessageTrigger) -> Self {
        grpc::AsyncMessageTrigger {
            address: value.address.to_string(),
            datastore_key: value.datastore_key,
        }
    }
}
