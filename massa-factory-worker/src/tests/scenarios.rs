use super::TestFactory;
use massa_models::{
    amount::Amount,
    operation::{Operation, OperationSerializer, OperationType},
    wrapped::WrappedContent,
};
use massa_signature::KeyPair;
use std::str::FromStr;
use std::time::Duration;

/// Creates a basic empty block with the factory.
#[test]
#[ignore]
fn basic_creation() {
    let keypair = KeyPair::generate();
    let mut test_factory = TestFactory::new(&keypair);
    let (block_id, storage) = test_factory.get_next_created_block(None, None);
    assert_eq!(block_id, storage.read_blocks().get(&block_id).unwrap().id);
}

/// Creates a block with a roll buy operation in it.
#[test]
#[ignore]
fn basic_creation_with_operation() {
    let keypair = KeyPair::generate();
    let mut test_factory = TestFactory::new(&keypair);

    let content = Operation {
        fee: Amount::from_str("0.01").unwrap(),
        expire_period: 2,
        op: OperationType::RollBuy { roll_count: 1 },
    };
    let operation = Operation::new_wrapped(content, OperationSerializer::new(), &keypair).unwrap();
    let (block_id, storage) = test_factory.get_next_created_block(Some(vec![operation]), None);

    let block = storage.read_blocks().get(&block_id).unwrap().clone();
    for op_id in block.content.operations.iter() {
        storage.read_operations().get(op_id).unwrap();
    }
    assert_eq!(block.content.operations.len(), 1);
}

/// Creates a block with a multiple operations in it.
#[test]
#[ignore]
fn basic_creation_with_multiple_operations() {
    let keypair = KeyPair::generate();
    let mut test_factory = TestFactory::new(&keypair);

    let content = Operation {
        fee: Amount::from_str("0.01").unwrap(),
        expire_period: 2,
        op: OperationType::RollBuy { roll_count: 1 },
    };
    let operation = Operation::new_wrapped(content, OperationSerializer::new(), &keypair).unwrap();
    let (block_id, storage) =
        test_factory.get_next_created_block(Some(vec![operation.clone(), operation]), None);

    let block = storage.read_blocks().get(&block_id).unwrap().clone();
    for op_id in block.content.operations.iter() {
        storage.read_operations().get(op_id).unwrap();
    }
    assert_eq!(block.content.operations.len(), 2);
}

use serial_test::serial;
use massa_hash::Hash;
use massa_models::block::{BlockHeader, BlockHeaderSerializer, BlockId};
use massa_models::config::THREAD_COUNT;
use massa_models::denunciation_interest::DenunciationInterest;
use massa_models::endorsement::{Endorsement, EndorsementSerializer, EndorsementSerializerLW, WrappedEndorsement};
use massa_models::denunciation::Denunciation;
use massa_models::slot::Slot;
use massa_models::wrapped::Wrapped;

/// Send 2 wrapped endorsements and check if a Denunciation op is in storage
#[test]
#[serial]
fn test_denunciation_factory_endorsement_denunciation() {
    let keypair = KeyPair::generate();
    let test_factory = TestFactory::new(&keypair);

    let sender_keypair = KeyPair::generate();

    let slot = Slot::new(3, 7);
    let endorsement1 = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
    };
    let wrapped_endorsement1: WrappedEndorsement = Endorsement::new_wrapped(
        endorsement1,
        EndorsementSerializer::new(),
        &sender_keypair,
    )
        .unwrap();

    let endorsement2 = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
    };
    let wrapped_endorsement2: WrappedEndorsement = Endorsement::new_wrapped(
        endorsement2,
        EndorsementSerializer::new(),
        &sender_keypair,
    )
        .unwrap();

    test_factory.de_items_tx.send(DenunciationInterest::WrappedEndorsement(wrapped_endorsement1.clone())).unwrap();
    test_factory.de_items_tx.send(DenunciationInterest::WrappedEndorsement(wrapped_endorsement2.clone())).unwrap();

    // Wait for denunciation factory to create the Denunciation
    std::thread::sleep(Duration::from_secs(1));

    // Get Operation from storage
    let op_ids_ = test_factory.storage.read_operations();
    let op_ids = op_ids_.get_operations();
    assert_eq!(op_ids.len(), 1);
    let op = op_ids.values().next().unwrap();

    match &op.content.op {
        OperationType::Denunciation { data: de } => {
            assert!(de.is_valid());
            assert_eq!(&Denunciation::from((&wrapped_endorsement1, &wrapped_endorsement2)), de);
        }
        _ => {
            panic!("Should be a Denunciation op and not: {:?}", op.content.op);
        }
    }

    // TODO: Testnet 17: Check Pool & Propagation

    // std::thread::sleep(Duration::from_secs(1));

    drop(op_ids_); // release RwLockReadGuard
    // stop everything
    drop(test_factory);
}

/// Send 2 block headers and check if a Denunciation op is in storage
#[test]
#[serial]
fn test_denunciation_factory_block_header_denunciation() {

    let keypair = KeyPair::generate();

    let slot = Slot::new(2, 1);
    let parents: Vec<BlockId> = (0..THREAD_COUNT)
        .map(|i| BlockId(Hash::compute_from(&[i])))
        .collect();

    let parents2: Vec<BlockId> = (0..THREAD_COUNT)
        .map(|i| BlockId(Hash::compute_from(&[i+1])))
        .collect();

    let header1 = BlockHeader {
        slot,
        parents: parents.clone(),
        operation_merkle_root: Hash::compute_from("mno".as_bytes()),
        endorsements: vec![
            Endorsement::new_wrapped(
                Endorsement {
                    slot: Slot::new(1, 1),
                    index: 1,
                    endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
                },
                EndorsementSerializerLW::new(),
                &keypair,
            )
                .unwrap(),
        ],
    };

    let wrapped_header1: Wrapped<BlockHeader, BlockId> = BlockHeader::new_wrapped(
        header1,
        BlockHeaderSerializer::new(),
        &keypair
    ).unwrap();

    let header2 = BlockHeader {
        slot,
        parents: parents2,
        operation_merkle_root: Hash::compute_from("mno".as_bytes()),
        endorsements: vec![
            Endorsement::new_wrapped(
                Endorsement {
                    slot: Slot::new(1, 1),
                    index: 1,
                    endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
                },
                EndorsementSerializerLW::new(),
                &keypair,
            )
                .unwrap(),
        ],
    };

    let wrapped_header2: Wrapped<BlockHeader, BlockId> = BlockHeader::new_wrapped(
        header2,
        BlockHeaderSerializer::new(),
        &keypair
    ).unwrap();

    let test_factory = TestFactory::new(&keypair);

    test_factory.de_items_tx.send(DenunciationInterest::WrappedHeader(wrapped_header1.clone())).unwrap();
    test_factory.de_items_tx.send(DenunciationInterest::WrappedHeader(wrapped_header2.clone())).unwrap();

    // Wait for denunciation factory to create the Denunciation
    std::thread::sleep(Duration::from_secs(1));

    // Get Operation from storage
    let op_ids_ = test_factory.storage.read_operations();
    let op_ids = op_ids_.get_operations();
    assert_eq!(op_ids.len(), 1);
    let op = op_ids.values().next().unwrap();

    match &op.content.op {
        OperationType::Denunciation { data: de } => {
            assert!(de.is_valid());
            assert_eq!(&Denunciation::from((&wrapped_header1, &wrapped_header2)), de);
        }
        _ => {
            panic!("Should be a Denunciation op and not: {:?}", op.content.op);
        }
    }

    // TODO: Testnet 17: Check Pool & Propagation

    // std::thread::sleep(Duration::from_secs(1));

    drop(op_ids_); // release RwLockReadGuard
    // stop everything
    drop(test_factory);

}
