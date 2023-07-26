// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{AsyncMessage, AsyncMessageTrigger, AsyncMessageUpdate};
use massa_ledger_exports::SetOrKeep;
use massa_proto_rs::massa::model::v1 as grpc_model;

impl From<AsyncMessage> for grpc_model::AsyncMessage {
    fn from(value: AsyncMessage) -> Self {
        grpc_model::AsyncMessage {
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
            hash: "".to_string(),
        }
    }
}

impl From<AsyncMessageUpdate> for grpc_model::AsyncMessageUpdate {
    fn from(value: AsyncMessageUpdate) -> Self {
        grpc_model::AsyncMessageUpdate {
            emission_slot: match value.emission_slot {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepSlot {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value.into()),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepSlot {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            emission_index: match value.emission_index {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepFixed64 {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepFixed64 {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            sender: match value.sender {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepString {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value.to_string()),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepString {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            destination: match value.destination {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepString {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value.to_string()),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepString {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            handler: match value.handler {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepString {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepString {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            max_gas: match value.max_gas {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepFixed64 {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepFixed64 {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            fee: match value.fee {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepFixed64 {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value.to_raw()),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepFixed64 {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            coins: match value.coins {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepFixed64 {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value.to_raw()),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepFixed64 {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            validity_start: match value.validity_start {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepSlot {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value.into()),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepSlot {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            validity_end: match value.validity_end {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepSlot {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value.into()),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepSlot {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            data: match value.data {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepBytes {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepBytes {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            trigger: match value.trigger {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepAsyncMessageTrigger {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: value.map(|trigger| trigger.into()),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepAsyncMessageTrigger {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            can_be_executed: match value.can_be_executed {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepBool {
                    r#type: grpc_model::AsyncPoolChangeType::Set as i32,
                    value: Some(value),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepBool {
                    r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                    value: None,
                }),
            },
            hash: Some(grpc_model::SetOrKeepString {
                r#type: grpc_model::AsyncPoolChangeType::Delete as i32,
                value: None,
            }),
        }
    }
}

impl From<AsyncMessageTrigger> for grpc_model::AsyncMessageTrigger {
    fn from(value: AsyncMessageTrigger) -> Self {
        grpc_model::AsyncMessageTrigger {
            address: value.address.to_string(),
            datastore_key: value.datastore_key,
        }
    }
}
