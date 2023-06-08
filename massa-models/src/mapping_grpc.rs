// Copyright (c) 2023 MASSA LABS <info@massa.net>

use std::str::FromStr;

use crate::address::Address;
use crate::block::{Block, FilledBlock, SecureShareBlock};
use crate::block_header::{BlockHeader, SecuredHeader};
use crate::denunciation::DenunciationIndex;
use crate::endorsement::{Endorsement, SecureShareEndorsement};
use crate::error::ModelsError;
use crate::execution::EventFilter;
use crate::operation::{Operation, OperationId, OperationType, SecureShareOperation};
use crate::output_event::{EventExecutionContext, SCOutputEvent};
use crate::slot::{IndexedSlot, Slot};
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1 as grpc_model;
use massa_signature::{PublicKey, Signature};

impl From<Block> for grpc_model::Block {
    fn from(value: Block) -> Self {
        grpc_model::Block {
            header: Some(value.header.into()),
            operations: value
                .operations
                .into_iter()
                .map(|operation| operation.to_string())
                .collect(),
        }
    }
}

impl From<BlockHeader> for grpc_model::BlockHeader {
    fn from(value: BlockHeader) -> Self {
        let res = value.endorsements.into_iter().map(|e| e.into()).collect();

        grpc_model::BlockHeader {
            slot: Some(value.slot.into()),
            parents: value
                .parents
                .into_iter()
                .map(|parent| parent.to_string())
                .collect(),
            operation_merkle_root: value.operation_merkle_root.to_string(),
            endorsements: res,
        }
    }
}

impl From<FilledBlock> for grpc_model::FilledBlock {
    fn from(value: FilledBlock) -> Self {
        grpc_model::FilledBlock {
            header: Some(value.header.into()),
            operations: value
                .operations
                .into_iter()
                .map(|tuple| grpc_model::FilledOperationTuple {
                    operation_id: tuple.0.to_string(),
                    operation: tuple.1.map(|op| op.into()),
                })
                .collect(),
        }
    }
}

impl From<SecureShareBlock> for grpc_model::SignedBlock {
    fn from(value: SecureShareBlock) -> Self {
        grpc_model::SignedBlock {
            content: Some(value.content.into()),
            signature: value.signature.to_bs58_check(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
        }
    }
}

impl From<SecuredHeader> for grpc_model::SignedBlockHeader {
    fn from(value: SecuredHeader) -> Self {
        grpc_model::SignedBlockHeader {
            content: Some(value.content.into()),
            signature: value.signature.to_bs58_check(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
        }
    }
}

impl From<Endorsement> for grpc_model::Endorsement {
    fn from(value: Endorsement) -> Self {
        grpc_model::Endorsement {
            slot: Some(value.slot.into()),
            index: value.index,
            endorsed_block: value.endorsed_block.to_string(),
        }
    }
}

impl From<SecureShareEndorsement> for grpc_model::SignedEndorsement {
    fn from(value: SecureShareEndorsement) -> Self {
        grpc_model::SignedEndorsement {
            content: Some(value.content.into()),
            signature: value.signature.to_bs58_check(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
        }
    }
}

impl From<OperationType> for grpc_model::OperationType {
    fn from(operation_type: OperationType) -> grpc_model::OperationType {
        let mut grpc_operation_type = grpc_model::OperationType::default();
        match operation_type {
            OperationType::Transaction {
                recipient_address,
                amount,
            } => {
                let transaction = grpc_model::Transaction {
                    recipient_address: recipient_address.to_string(),
                    amount: amount.to_raw(),
                };
                grpc_operation_type.r#type =
                    Some(grpc_model::operation_type::Type::Transaction(transaction));
            }
            OperationType::RollBuy { roll_count } => {
                let roll_buy = grpc_model::RollBuy { roll_count };
                grpc_operation_type.roll_buy = Some(roll_buy);
            }
            OperationType::RollSell { roll_count } => {
                let roll_sell = grpc_model::RollSell { roll_count };
                grpc_operation_type.roll_sell = Some(roll_sell);
            }
            OperationType::ExecuteSC {
                data,
                max_gas,
                max_coins,
                datastore,
            } => {
                let execute_sc = grpc_model::ExecuteSc {
                    data,
                    max_coins: max_coins.to_raw(),
                    max_gas,
                    datastore: datastore
                        .into_iter()
                        .map(|(key, value)| grpc_model::BytesMapFieldEntry { key, value })
                        .collect(),
                };
                grpc_operation_type.r#type =
                    Some(grpc_model::operation_type::Type::ExecutSc(execute_sc));
            }
            OperationType::CallSC {
                target_addr,
                target_func,
                param,
                max_gas,
                coins,
            } => {
                let call_sc = grpc_model::CallSc {
                    target_addr: target_addr.to_string(),
                    target_func,
                    param,
                    max_gas,
                    coins: coins.to_raw(),
                };
                grpc_operation_type.r#type =
                    Some(grpc_model::operation_type::Type::CallSc(call_sc));
            }
        }

        grpc_operation_type
    }
}

impl From<Operation> for grpc_model::Operation {
    fn from(op: Operation) -> Self {
        grpc_model::Operation {
            fee: op.fee.to_raw(),
            expire_period: op.expire_period,
            op: Some(op.op.into()),
        }
    }
}

impl From<OperationType> for grpc_api::OpType {
    fn from(value: OperationType) -> Self {
        match value {
            OperationType::Transaction { .. } => grpc_api::OpType::Transaction,
            OperationType::RollBuy { .. } => grpc_api::OpType::RollBuy,
            OperationType::RollSell { .. } => grpc_api::OpType::RollSell,
            OperationType::ExecuteSC { .. } => grpc_api::OpType::ExecuteSc,
            OperationType::CallSC { .. } => grpc_api::OpType::CallSc,
        }
    }
}

impl From<SecureShareOperation> for grpc_model::SignedOperation {
    fn from(value: SecureShareOperation) -> Self {
        grpc_model::SignedOperation {
            content: Some(value.content.into()),
            signature: value.signature.to_bs58_check(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
        }
    }
}

impl From<IndexedSlot> for grpc_model::IndexedSlot {
    fn from(s: IndexedSlot) -> Self {
        grpc_model::IndexedSlot {
            index: s.index as u64,
            slot: Some(s.slot.into()),
        }
    }
}

impl From<Slot> for grpc_model::Slot {
    fn from(s: Slot) -> Self {
        grpc_model::Slot {
            period: s.period,
            thread: s.thread as u32,
        }
    }
}

impl From<grpc_model::Slot> for Slot {
    fn from(s: grpc_model::Slot) -> Self {
        Slot {
            period: s.period,
            thread: s.thread as u8,
        }
    }
}

impl TryFrom<grpc_api::GetScExecutionEventsFilter> for EventFilter {
    type Error = crate::error::ModelsError;

    fn try_from(filter: grpc_api::GetScExecutionEventsFilter) -> Result<Self, Self::Error> {
        let status_final = grpc_model::ScExecutionEventStatus::Final as i32;
        let status_error = grpc_model::ScExecutionEventStatus::Failure as i32;
        Ok(Self {
            start: filter.start_slot.map(|slot| slot.into()),
            end: filter.end_slot.map(|slot| slot.into()),
            emitter_address: filter
                .emitter_address
                .map(|address| Address::from_str(&address))
                .transpose()?,
            original_caller_address: filter
                .caller_address
                .map(|address| Address::from_str(&address))
                .transpose()?,
            original_operation_id: filter
                .original_operation_id
                .map(|operation_id| OperationId::from_str(&operation_id))
                .transpose()?,
            is_final: Some(filter.status.contains(&status_final)),
            is_error: Some(filter.status.contains(&status_error)),
        })
    }
}

impl From<SCOutputEvent> for grpc_model::ScExecutionEvent {
    fn from(value: SCOutputEvent) -> Self {
        grpc_model::ScExecutionEvent {
            context: Some(value.context.into()),
            data: value.data,
        }
    }
}

impl From<EventExecutionContext> for grpc_model::ScExecutionEventContext {
    fn from(value: EventExecutionContext) -> Self {
        let id_str = format!(
            "{}{}{}",
            &value.slot.period, &value.slot.thread, &value.index_in_slot
        );
        let id = bs58::encode(id_str.as_bytes()).with_check().into_string();
        Self {
            id,
            origin_slot: Some(value.slot.into()),
            block_id: value.block.map(|id| id.to_string()),
            index_in_slot: value.index_in_slot,
            call_stack: value
                .call_stack
                .into_iter()
                .map(|a| a.to_string())
                .collect(),
            origin_operation_id: value.origin_operation_id.map(|id| id.to_string()),
            status: {
                let mut status = Vec::new();
                if value.read_only {
                    status.push(grpc_model::ScExecutionEventStatus::ReadOnly as i32);
                }
                if value.is_error {
                    status.push(grpc_model::ScExecutionEventStatus::Failure as i32);
                }
                if value.is_final {
                    status.push(grpc_model::ScExecutionEventStatus::Final as i32);
                }

                status
            },
        }
    }
}

impl From<DenunciationIndex> for grpc_model::DenunciationIndex {
    fn from(value: DenunciationIndex) -> Self {
        grpc_model::DenunciationIndex {
            entry: Some(match value {
                DenunciationIndex::BlockHeader { slot } => {
                    grpc_model::denunciation_index::Entry::BlockHeader(
                        grpc_model::DenunciationBlockHeader {
                            slot: Some(slot.into()),
                        },
                    )
                }
                DenunciationIndex::Endorsement { slot, index } => {
                    grpc_model::denunciation_index::Entry::Endorsement(
                        grpc_model::DenunciationEndorsement {
                            slot: Some(slot.into()),
                            index,
                        },
                    )
                }
            }),
        }
    }
}

/// Converts a gRPC `SecureShare` into a byte vector
pub fn secure_share_to_vec(value: grpc_model::SecureShare) -> Result<Vec<u8>, ModelsError> {
    let pub_key = PublicKey::from_str(&value.content_creator_pub_key)?;
    let pub_key_b = pub_key.to_bytes();
    // Concatenate signature, public key, and data into a single byte vector
    let mut serialized_content =
        Vec::with_capacity(value.signature.len() + pub_key_b.len() + value.serialized_data.len());
    serialized_content
        .extend_from_slice(&Signature::from_str(&value.signature).map(|value| value.to_bytes())?);
    serialized_content.extend_from_slice(&pub_key_b);
    serialized_content.extend_from_slice(&value.serialized_data);

    Ok(serialized_content)
}
