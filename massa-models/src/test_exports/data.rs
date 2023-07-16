use massa_hash::Hash;
use massa_signature::KeyPair;

use crate::block_header::{BlockHeader, BlockHeaderSerializer, SecuredHeader};
use crate::block_id::BlockId;
use crate::config::THREAD_COUNT;
use crate::endorsement::{
    Endorsement, EndorsementSerializer, EndorsementSerializerLW, SecureShareEndorsement,
};
use crate::secure_share::SecureShareContent;
use crate::slot::Slot;

/// Helper to generate endorsements ready for denunciation
pub fn gen_endorsements_for_denunciation(
    with_slot: Option<Slot>,
    with_keypair: Option<KeyPair>,
) -> (
    Slot,
    KeyPair,
    SecureShareEndorsement,
    SecureShareEndorsement,
    SecureShareEndorsement,
) {
    let keypair = with_keypair.unwrap_or(KeyPair::generate(0).unwrap());
    let slot = with_slot.unwrap_or(Slot::new(3, 7));

    let endorsement_1 = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
    };

    let v_endorsement1 =
        Endorsement::new_verifiable(endorsement_1, EndorsementSerializer::new(), &keypair).unwrap();

    let endorsement_2 = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
    };

    let v_endorsement2 =
        Endorsement::new_verifiable(endorsement_2, EndorsementSerializer::new(), &keypair).unwrap();

    let endorsement_3 = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blk3".as_bytes())),
    };
    let v_endorsement_3 =
        Endorsement::new_verifiable(endorsement_3, EndorsementSerializer::new(), &keypair).unwrap();

    return (
        slot,
        keypair,
        v_endorsement1,
        v_endorsement2,
        v_endorsement_3,
    );
}

/// Helper to generate block headers ready for denunciation
pub fn gen_block_headers_for_denunciation(
    with_slot: Option<Slot>,
    with_keypair: Option<KeyPair>,
) -> (Slot, KeyPair, SecuredHeader, SecuredHeader, SecuredHeader) {
    let keypair = with_keypair.unwrap_or(KeyPair::generate(0).unwrap());
    let slot = with_slot.unwrap_or(Slot::new(2, 1));

    let parents_1: Vec<BlockId> = (0..THREAD_COUNT)
        .map(|i| BlockId(Hash::compute_from(&[i])))
        .collect();
    let parents_2: Vec<BlockId> = (0..THREAD_COUNT)
        .map(|i| BlockId(Hash::compute_from(&[i + 1])))
        .collect();
    let parents_3: Vec<BlockId> = (0..THREAD_COUNT)
        .map(|i| BlockId(Hash::compute_from(&[i + 2])))
        .collect();

    let endorsement_1 = Endorsement {
        slot: Slot::new(1, 1),
        index: 1,
        endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
    };
    let s_endorsement_1 =
        Endorsement::new_verifiable(endorsement_1, EndorsementSerializerLW::new(), &keypair)
            .unwrap();

    let block_header_1 = BlockHeader {
        current_version: 0,
        announced_version: None,
        slot,
        parents: parents_1,
        operation_hash: Hash::compute_from("mno".as_bytes()),
        endorsements: vec![s_endorsement_1.clone()],
        denunciations: vec![],
    };

    // create header
    let s_block_header_1 = BlockHeader::new_verifiable::<BlockHeaderSerializer, BlockId>(
        block_header_1,
        BlockHeaderSerializer::new(),
        &keypair,
    )
    .expect("error while producing block header");

    let block_header_2 = BlockHeader {
        current_version: 0,
        announced_version: None,
        slot,
        parents: parents_2,
        operation_hash: Hash::compute_from("mno".as_bytes()),
        endorsements: vec![s_endorsement_1.clone()],
        denunciations: vec![],
    };

    // create header
    let s_block_header_2 = BlockHeader::new_verifiable::<BlockHeaderSerializer, BlockId>(
        block_header_2,
        BlockHeaderSerializer::new(),
        &keypair,
    )
    .expect("error while producing block header");

    let block_header_3 = BlockHeader {
        current_version: 0,
        announced_version: None,
        slot,
        parents: parents_3,
        operation_hash: Hash::compute_from("mno".as_bytes()),
        endorsements: vec![s_endorsement_1.clone()],
        denunciations: vec![],
    };

    // create header
    let s_block_header_3 = BlockHeader::new_verifiable::<BlockHeaderSerializer, BlockId>(
        block_header_3,
        BlockHeaderSerializer::new(),
        &keypair,
    )
    .expect("error while producing block header");

    return (
        slot,
        keypair,
        s_block_header_1.clone(),
        s_block_header_2,
        s_block_header_3,
    );
}
