use crate::block_header::{BlockHeader, SecuredHeader};
use crate::endorsement::{Endorsement, SecureShareEndorsement};
use crate::slot::{IndexedSlot, Slot};
use massa_proto::massa::api::v1::{self as grpc};

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

impl From<Endorsement> for grpc::Endorsement {
    fn from(value: Endorsement) -> Self {
        grpc::Endorsement {
            slot: Some(value.slot.into()),
            index: value.index,
            endorsed_block: value.endorsed_block.to_string(),
        }
    }
}

impl From<SecureShareEndorsement> for grpc::SecureShareEndorsement {
    fn from(value: SecureShareEndorsement) -> Self {
        grpc::SecureShareEndorsement {
            content: Some(value.content.into()),
            signature: value.signature.to_string(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
        }
    }
}

impl From<BlockHeader> for grpc::BlockHeader {
    fn from(value: BlockHeader) -> Self {
        let mut res = vec![];
        for endorsement in value.endorsements {
            res.push(endorsement.into());
        }

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

impl From<SecuredHeader> for grpc::SecureShareBlockHeader {
    fn from(value: SecuredHeader) -> Self {
        grpc::SecureShareBlockHeader {
            signature: value.signature.to_string(),
            content_creator_pub_key: value.content_creator_pub_key.to_string(),
            content_creator_address: value.content_creator_address.to_string(),
            id: value.id.to_string(),
            content: Some(value.content.into()),
        }
    }
}
