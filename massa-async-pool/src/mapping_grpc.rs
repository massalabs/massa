// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{AsyncMessage, AsyncMessageTrigger, AsyncMessageUpdate};
use massa_ledger_exports::SetOrKeep;
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

impl From<AsyncMessageUpdate> for grpc::AsyncMessageUpdate {
    fn from(value: AsyncMessageUpdate) -> Self {
        grpc::AsyncMessageUpdate {
            emission_slot: match value.emission_slot {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepSlot {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value.into()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepSlot {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            emission_index: match value.emission_index {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepFixed64 {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepFixed64 {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            sender: match value.sender {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepString {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value.to_string()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepString {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            destination: match value.destination {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepString {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value.to_string()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepString {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            handler: match value.handler {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepString {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepString {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            max_gas: match value.max_gas {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepFixed64 {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepFixed64 {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            fee: match value.fee {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepFixed64 {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value.to_raw()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepFixed64 {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            coins: match value.coins {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepFixed64 {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value.to_raw()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepFixed64 {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            validity_start: match value.validity_start {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepSlot {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value.into()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepSlot {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            validity_end: match value.validity_end {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepSlot {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value.into()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepSlot {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            data: match value.data {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepBytes {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepBytes {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            trigger: match value.trigger {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepAsyncMessageTrigger {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: value.map(|trigger| trigger.into()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepAsyncMessageTrigger {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            can_be_executed: match value.can_be_executed {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepBool {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepBool {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            hash: match value.hash {
                SetOrKeep::Set(value) => Some(grpc::SetOrKeepString {
                    r#type: grpc::AsyncPoolChangeType::Set as i32,
                    value: Some(value.to_string()),
                }),
                SetOrKeep::Keep => Some(grpc::SetOrKeepString {
                    r#type: grpc::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
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
