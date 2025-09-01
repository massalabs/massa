// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::amount::Amount;
use crate::async_msg::{AsyncMessage, AsyncMessageTrigger, AsyncMessageUpdate};
use crate::block::{Block, BlockGraphStatus, FilledBlock, SecureShareBlock};
use crate::block_header::{BlockHeader, SecuredHeader};
use crate::config::CompactConfig;
use crate::denunciation::{Denunciation, DenunciationIndex};
use crate::endorsement::{Endorsement, SecureShareEndorsement};
use crate::error::ModelsError;
use crate::operation::{Operation, OperationType, SecureShareOperation};
use crate::output_event::{EventExecutionContext, SCOutputEvent};
use crate::slot::{IndexedSlot, Slot};
use crate::stats::{ConsensusStats, ExecutionStats, NetworkStats};
use crate::types::SetOrKeep;
use massa_proto_rs::massa::model::v1 as grpc_model;

//TODO check error type
/// Converts a gRPC `grpc_model::DenunciationIndex` into a DenunciationIndex
pub fn to_denunciation_index(
    value: grpc_model::DenunciationIndex,
) -> Result<DenunciationIndex, ModelsError> {
    match value
        .entry
        .ok_or_else(|| ModelsError::ErrorRaised("no denunciation_index found".to_string()))?
    {
        grpc_model::denunciation_index::Entry::BlockHeader(block_header) => {
            Ok(DenunciationIndex::BlockHeader {
                slot: block_header
                    .slot
                    .ok_or_else(|| ModelsError::ErrorRaised("no slot found".to_string()))?
                    .into(),
            })
        }
        grpc_model::denunciation_index::Entry::Endorsement(endorsement) => {
            Ok(DenunciationIndex::Endorsement {
                slot: endorsement
                    .slot
                    .ok_or_else(|| ModelsError::ErrorRaised("no slot found".to_string()))?
                    .into(),
                index: endorsement.index,
            })
        }
    }
}

impl From<Amount> for grpc_model::NativeAmount {
    fn from(value: Amount) -> Self {
        let (mantissa, scale) = value.to_mantissa_scale();
        grpc_model::NativeAmount { mantissa, scale }
    }
}

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

impl From<BlockGraphStatus> for i32 {
    fn from(value: BlockGraphStatus) -> Self {
        match value {
            BlockGraphStatus::ActiveInBlockclique => {
                grpc_model::BlockStatus::NonFinalBlockclique.into()
            }
            BlockGraphStatus::ActiveInAlternativeCliques => {
                grpc_model::BlockStatus::NonFinalAlternateClique.into()
            }
            BlockGraphStatus::Final => grpc_model::BlockStatus::Final.into(),
            BlockGraphStatus::Discarded => grpc_model::BlockStatus::Discarded.into(),
            _ => grpc_model::BlockStatus::Unspecified.into(),
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
            endorsements: res,
            denunciations: value.denunciations.into_iter().map(|d| d.into()).collect(),
            current_version: value.current_version,
            announced_version: value.announced_version,
            //TODO to be updated in Massa models
            operations_hash: value.operation_merkle_root.to_string(),
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
                .map(|tuple| grpc_model::FilledOperationEntry {
                    operation_id: tuple.0.to_string(),
                    operation: tuple.1.map(|op| op.into()),
                })
                .collect(),
        }
    }
}

impl From<SecureShareBlock> for grpc_model::SignedBlock {
    fn from(value: SecureShareBlock) -> Self {
        let serialized_size = value.serialized_size() as u64;
        grpc_model::SignedBlock {
            content: Some(value.content.into()),
            signature: value.signature.to_string(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            secure_hash: value.id.to_string(),
            serialized_size,
        }
    }
}

impl From<SecuredHeader> for grpc_model::SignedBlockHeader {
    fn from(value: SecuredHeader) -> Self {
        let serialized_size = value.serialized_size() as u64;
        grpc_model::SignedBlockHeader {
            content: Some(value.content.into()),
            signature: value.signature.to_string(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            secure_hash: value.id.to_string(),
            serialized_size,
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
        let serialized_size = value.serialized_size() as u64;
        grpc_model::SignedEndorsement {
            content: Some(value.content.into()),
            signature: value.signature.to_string(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            secure_hash: value.id.to_string(),
            serialized_size,
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
                    amount: Some(amount.into()),
                };
                grpc_operation_type.r#type =
                    Some(grpc_model::operation_type::Type::Transaction(transaction));
            }
            OperationType::RollBuy { roll_count } => {
                let roll_buy = grpc_model::RollBuy { roll_count };
                grpc_operation_type.r#type =
                    Some(grpc_model::operation_type::Type::RollBuy(roll_buy));
            }
            OperationType::RollSell { roll_count } => {
                let roll_sell = grpc_model::RollSell { roll_count };
                grpc_operation_type.r#type =
                    Some(grpc_model::operation_type::Type::RollSell(roll_sell));
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
                    Some(grpc_model::operation_type::Type::ExecuteSc(execute_sc));
            }
            OperationType::CallSC {
                target_addr,
                target_func,
                param,
                max_gas,
                coins,
            } => {
                let call_sc = grpc_model::CallSc {
                    target_address: target_addr.to_string(),
                    target_function: target_func,
                    parameter: param,
                    max_gas,
                    coins: Some(coins.into()),
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
            fee: Some(op.fee.into()),
            expire_period: op.expire_period,
            op: Some(op.op.into()),
        }
    }
}

impl From<OperationType> for grpc_model::OpType {
    fn from(value: OperationType) -> Self {
        match value {
            OperationType::Transaction { .. } => grpc_model::OpType::Transaction,
            OperationType::RollBuy { .. } => grpc_model::OpType::RollBuy,
            OperationType::RollSell { .. } => grpc_model::OpType::RollSell,
            OperationType::ExecuteSC { .. } => grpc_model::OpType::ExecuteSc,
            OperationType::CallSC { .. } => grpc_model::OpType::CallSc,
        }
    }
}

impl From<SecureShareOperation> for grpc_model::SignedOperation {
    fn from(value: SecureShareOperation) -> Self {
        grpc_model::SignedOperation {
            serialized_size: value.serialized_size() as u64,
            content: Some(value.content.into()),
            signature: value.signature.to_string(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            secure_hash: value.id.to_string(),
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

impl From<SCOutputEvent> for grpc_model::ScExecutionEvent {
    fn from(value: SCOutputEvent) -> Self {
        grpc_model::ScExecutionEvent {
            context: Some(value.context.into()),
            data: value.data.as_bytes().to_vec(),
        }
    }
}

impl From<EventExecutionContext> for grpc_model::ScExecutionEventContext {
    fn from(value: EventExecutionContext) -> Self {
        Self {
            origin_slot: Some(value.slot.into()),
            block_id: value.block.map(|id| id.to_string()),
            index_in_slot: value.index_in_slot,
            call_stack: value
                .call_stack
                .into_iter()
                .map(|a| a.to_string())
                .collect(),
            origin_operation_id: value.origin_operation_id.map(|id| id.to_string()),
            is_failure: value.is_error,
            status: if value.read_only {
                grpc_model::ScExecutionEventStatus::ReadOnly as i32
            } else if value.is_final {
                grpc_model::ScExecutionEventStatus::Final as i32
            } else {
                grpc_model::ScExecutionEventStatus::Unspecified as i32
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

impl From<CompactConfig> for grpc_model::CompactConfig {
    fn from(value: CompactConfig) -> Self {
        grpc_model::CompactConfig {
            genesis_timestamp: Some(value.genesis_timestamp.into()),
            end_timestamp: value.end_timestamp.map(|time| time.into()),
            thread_count: value.thread_count as u32,
            t0: Some(value.t0.into()),
            delta_f0: value.delta_f0,
            operation_validity_periods: value.operation_validity_periods,
            periods_per_cycle: value.periods_per_cycle,
            block_reward: Some(value.block_reward.into()),
            roll_price: Some(value.roll_price.into()),
            max_block_size: value.max_block_size,
        }
    }
}

impl From<ConsensusStats> for grpc_model::ConsensusStats {
    fn from(value: ConsensusStats) -> Self {
        grpc_model::ConsensusStats {
            start_timespan: Some(value.start_timespan.into()),
            end_timespan: Some(value.end_timespan.into()),
            final_block_count: value.final_block_count,
            stale_block_count: value.stale_block_count,
            clique_count: value.clique_count,
        }
    }
}

impl From<ExecutionStats> for grpc_model::ExecutionStats {
    fn from(value: ExecutionStats) -> Self {
        grpc_model::ExecutionStats {
            time_window_start: Some(value.time_window_start.into()),
            time_window_end: Some(value.time_window_end.into()),
            final_block_count: value.final_block_count as u64,
            final_executed_operations_count: value.final_executed_operations_count as u64,
        }
    }
}

impl From<NetworkStats> for grpc_model::NetworkStats {
    fn from(value: NetworkStats) -> Self {
        grpc_model::NetworkStats {
            in_connection_count: value.in_connection_count,
            out_connection_count: value.out_connection_count,
            known_peer_count: value.known_peer_count,
            banned_peer_count: value.banned_peer_count,
            active_node_count: value.active_node_count,
        }
    }
}

impl From<Denunciation> for grpc_model::Denunciation {
    fn from(value: Denunciation) -> Self {
        match value {
            Denunciation::Endorsement(de) => grpc_model::Denunciation {
                entry: Some(grpc_model::denunciation::Entry::Endorsement(
                    grpc_model::EndorsementDenunciation {
                        public_key: de.get_public_key().to_string(),
                        slot: Some((*de.get_slot()).into()),
                        index: *de.get_index(),
                        hash_1: de.get_hash_1().to_string(),
                        hash_2: de.get_hash_2().to_string(),
                        signature_1: de.get_signature_1().to_string(),
                        signature_2: de.get_signature_2().to_string(),
                    },
                )),
            },
            Denunciation::BlockHeader(db) => grpc_model::Denunciation {
                entry: Some(grpc_model::denunciation::Entry::BlockHeader(
                    grpc_model::BlockHeaderDenunciation {
                        public_key: db.get_public_key().to_string(),
                        slot: Some((*db.get_slot()).into()),
                        hash_1: db.get_hash_1().to_string(),
                        hash_2: db.get_hash_2().to_string(),
                        signature_1: db.get_signature_1().to_string(),
                        signature_2: db.get_signature_2().to_string(),
                    },
                )),
            },
        }
    }
}

impl From<AsyncMessage> for grpc_model::AsyncMessage {
    fn from(value: AsyncMessage) -> Self {
        grpc_model::AsyncMessage {
            emission_slot: Some(value.emission_slot.into()),
            emission_index: value.emission_index,
            sender: value.sender.to_string(),
            destination: value.destination.to_string(),
            handler: value.function.to_string(),
            max_gas: value.max_gas,
            fee: Some(value.fee.into()),
            coins: Some(value.coins.into()),
            validity_start: Some(value.validity_start.into()),
            validity_end: Some(value.validity_start.into()),
            data: value.function_params,
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
            handler: match value.function {
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
            data: match value.function_params {
                SetOrKeep::Set(value) => Some(grpc_model::SetOrKeepBytes {
                    change: Some(grpc_model::set_or_keep_bytes::Change::Set(value)),
                }),
                SetOrKeep::Keep => Some(grpc_model::SetOrKeepBytes {
                    change: Some(grpc_model::set_or_keep_bytes::Change::Keep(
                        grpc_model::Empty {},
                    )),
                }),
            },
            trigger: match value.trigger {
                SetOrKeep::Set(value) => match value {
                    None => Some(grpc_model::SetOrKeepAsyncMessageTrigger { change: None }),
                    Some(trigger) => Some(grpc_model::SetOrKeepAsyncMessageTrigger {
                        change: Some(grpc_model::set_or_keep_async_message_trigger::Change::Set(
                            trigger.into(),
                        )),
                    }),
                },
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
