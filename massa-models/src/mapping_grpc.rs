use crate::block::{Block, FilledBlock, SecureShareBlock};
use crate::block_header::{BlockHeader, SecuredHeader};
use crate::endorsement::{Endorsement, SecureShareEndorsement};
use crate::operation::{Operation, OperationType, SecureShareOperation};
use crate::slot::{IndexedSlot, Slot};
use massa_proto::massa::api::v1::{self as grpc, BytesMapFieldEntry, FilledOperationTuple};

impl From<Block> for grpc::Block {
    fn from(value: Block) -> Self {
        grpc::Block {
            header: Some(value.header.into()),
            operations: value
                .operations
                .into_iter()
                .map(|operation| operation.to_string())
                .collect(),
        }
    }
}

impl From<BlockHeader> for grpc::BlockHeader {
    fn from(value: BlockHeader) -> Self {
        let res = value.endorsements.into_iter().map(|e| e.into()).collect();

        grpc::BlockHeader {
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

impl From<FilledBlock> for grpc::FilledBlock {
    fn from(value: FilledBlock) -> Self {
        grpc::FilledBlock {
            header: Some(value.header.into()),
            operations: value
                .operations
                .into_iter()
                .map(|tuple| FilledOperationTuple {
                    operation_id: tuple.0.to_string(),
                    operation: tuple.1.map(|op| op.into()),
                })
                .collect(),
        }
    }
}

impl From<SecureShareBlock> for grpc::SignedBlock {
    fn from(value: SecureShareBlock) -> Self {
        grpc::SignedBlock {
            content: Some(value.content.into()),
            signature: value.signature.to_bs58_check(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
        }
    }
}

impl From<SecuredHeader> for grpc::SignedBlockHeader {
    fn from(value: SecuredHeader) -> Self {
        grpc::SignedBlockHeader {
            content: Some(value.content.into()),
            signature: value.signature.to_bs58_check(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
        }
    }
}

impl From<Endorsement> for grpc::Endorsement {
    fn from(value: Endorsement) -> Self {
        grpc::Endorsement {
            slot: Some(value.slot.into()),
            index: value.index,
            endorsed_block: value.endorsed_block.to_string(),
        }
    }
}

impl From<SecureShareEndorsement> for grpc::SignedEndorsement {
    fn from(value: SecureShareEndorsement) -> Self {
        grpc::SignedEndorsement {
            content: Some(value.content.into()),
            signature: value.signature.to_bs58_check(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
        }
    }
}

//TODO to be deep checked before release
impl From<OperationType> for grpc::OperationType {
    fn from(operation_type: OperationType) -> grpc::OperationType {
        let mut grpc_operation_type = grpc::OperationType::default();
        match operation_type {
            OperationType::Transaction {
                recipient_address,
                amount,
            } => {
                let transaction = grpc::Transaction {
                    recipient_address: recipient_address.to_string(),
                    amount: amount.to_raw(),
                };
                grpc_operation_type.transaction = Some(transaction);
            }
            OperationType::RollBuy { roll_count } => {
                let roll_buy = grpc::RollBuy { roll_count };
                grpc_operation_type.roll_buy = Some(roll_buy);
            }
            OperationType::RollSell { roll_count } => {
                let roll_sell = grpc::RollSell { roll_count };
                grpc_operation_type.roll_sell = Some(roll_sell);
            }
            OperationType::ExecuteSC {
                data,
                max_gas,
                datastore,
            } => {
                let execute_sc = grpc::ExecuteSc {
                    data,
                    max_gas,
                    datastore: datastore
                        .into_iter()
                        .map(|(key, value)| BytesMapFieldEntry { key, value })
                        .collect(),
                };
                grpc_operation_type.execut_sc = Some(execute_sc);
            }
            OperationType::CallSC {
                target_addr,
                target_func,
                param,
                max_gas,
                coins,
            } => {
                let call_sc = grpc::CallSc {
                    target_addr: target_addr.to_string(),
                    target_func,
                    param,
                    max_gas,
                    coins: coins.to_raw(),
                };
                grpc_operation_type.call_sc = Some(call_sc);
            }
        }

        grpc_operation_type
    }
}

impl From<Operation> for grpc::Operation {
    fn from(op: Operation) -> Self {
        grpc::Operation {
            fee: op.fee.to_raw(),
            expire_period: op.expire_period,
            op: Some(op.op.into()),
        }
    }
}

impl From<SecureShareOperation> for grpc::SignedOperation {
    fn from(value: SecureShareOperation) -> Self {
        grpc::SignedOperation {
            content: Some(value.content.into()),
            signature: value.signature.to_bs58_check(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
        }
    }
}

impl From<IndexedSlot> for grpc::IndexedSlot {
    fn from(s: IndexedSlot) -> Self {
        grpc::IndexedSlot {
            index: s.index as u64,
            slot: Some(s.slot.into()),
        }
    }
}

impl From<Slot> for grpc::Slot {
    fn from(s: Slot) -> Self {
        grpc::Slot {
            period: s.period,
            thread: s.thread as u32,
        }
    }
}
