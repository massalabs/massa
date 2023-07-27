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
            fee: Some(value.fee.into()),
            coins: Some(value.coins.into()),
            validity_start: Some(value.validity_start.into()),
            validity_end: Some(value.validity_start.into()),
            data: value.data,
            trigger: value.trigger.map(|trigger| trigger.into()),
            can_be_executed: value.can_be_executed,
        }
    }
}

//TODO to be checked, use functions
impl From<AsyncMessageUpdate> for grpc_model::AsyncMessageUpdate {
    fn from(value: AsyncMessageUpdate) -> Self {
        grpc_model::AsyncMessageUpdate {
            emission_slot: match value.emission_slot {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepSlot {
                    change: Some(grpc_model::set_or_keep_slot::Change::Set(value.into())),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepSlot {
                    change: Some(grpc_model::set_or_keep_slot::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            emission_index: match value.emission_index {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepUint64 {
                    change: Some(grpc_model::set_or_keep_uint64::Change::Set(value)),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepUint64 {
                    change: Some(grpc_model::set_or_keep_uint64::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            sender: match value.sender {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepString {
                    change: Some(grpc_model::set_or_keep_string::Change::Set(
                        value.to_string(),
                    )),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepString {
                    change: Some(grpc_model::set_or_keep_string::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            destination: match value.destination {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepString {
                    change: Some(grpc_model::set_or_keep_string::Change::Set(
                        value.to_string(),
                    )),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepString {
                    change: Some(grpc_model::set_or_keep_string::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            handler: match value.handler {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepString {
                    change: Some(grpc_model::set_or_keep_string::Change::Set(value)),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepString {
                    change: Some(grpc_model::set_or_keep_string::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            max_gas: match value.max_gas {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepUint64 {
                    change: Some(grpc_model::set_or_keep_uint64::Change::Set(value)),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepUint64 {
                    change: Some(grpc_model::set_or_keep_uint64::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            //TODO check Amount usage
            fee: match value.fee {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepUint64 {
                    change: Some(grpc_model::set_or_keep_uint64::Change::Set(value.to_raw())),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepUint64 {
                    change: Some(grpc_model::set_or_keep_uint64::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            //TODO check Amount usage
            coins: match value.coins {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepUint64 {
                    change: Some(grpc_model::set_or_keep_uint64::Change::Set(value.to_raw())),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepUint64 {
                    change: Some(grpc_model::set_or_keep_uint64::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            validity_start: match value.validity_start {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepSlot {
                    change: Some(grpc_model::set_or_keep_slot::Change::Set(value.into())),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepSlot {
                    change: Some(grpc_model::set_or_keep_slot::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            validity_end: match value.validity_end {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepSlot {
                    change: Some(grpc_model::set_or_keep_slot::Change::Set(value.into())),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepSlot {
                    change: Some(grpc_model::set_or_keep_slot::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            data: match value.data {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepBytes {
                    change: Some(grpc_model::set_or_keep_bytes::Change::Set(value)),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepBytes {
                    change: Some(grpc_model::set_or_keep_bytes::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            //TODO remove unwrap
            trigger: match value.trigger {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepAsyncMessageTrigger {
                    change: Some(grpc_model::set_or_keep_async_message_trigger::Change::Set(
                        value.map(|trigger| trigger.into()).unwrap(),
                    )),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepAsyncMessageTrigger {
                    change: Some(grpc_model::set_or_keep_async_message_trigger::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            can_be_executed: match value.can_be_executed {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepBool {
                    change: Some(grpc_model::set_or_keep_bool::Change::Set(value)),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepBool {
                    change: Some(grpc_model::set_or_keep_bool::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
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
