use super::TestFactory;
use massa_models::{
    amount::Amount,
    operation::{Operation, OperationSerializer, OperationType},
    wrapped::WrappedContent,
};
use massa_signature::KeyPair;
use std::str::FromStr;

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
use massa_models::block::BlockId;
use massa_models::denunciation_interest::DenunciationInterest;
use massa_models::endorsement::{Endorsement, EndorsementHasher, EndorsementSerializer, WrappedEndorsement};
use massa_models::denunciation::Denunciation;
use massa_models::operation::WrappedOperation;
use massa_models::slot::Slot;

///
#[test]
#[serial]
fn test_denunciation_factory_endorsement_denunciation() {
    let keypair = KeyPair::generate();
    let mut test_factory = TestFactory::new(&keypair);

    let sender_keypair = KeyPair::generate();

    let slot = Slot::new(3, 7);
    let endorsement1 = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
    };
    let wrapped_endorsement1: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
        endorsement1,
        EndorsementSerializer::new(),
        &sender_keypair,
        EndorsementHasher::new(),
    )
        .unwrap();

    let endorsement2 = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
    };
    let wrapped_endorsement2: WrappedEndorsement = Endorsement::new_wrapped_with_hasher(
        endorsement2,
        EndorsementSerializer::new(),
        &sender_keypair,
        EndorsementHasher::new(),
    )
        .unwrap();

    test_factory.de_items_tx.send(DenunciationInterest::WrappedEndorsement(wrapped_endorsement1.clone())).unwrap();
    test_factory.de_items_tx.send(DenunciationInterest::WrappedEndorsement(wrapped_endorsement2.clone())).unwrap();

    test_factory.wait_until_next_slot();

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

    drop(op_ids_); // release RwLockReadGuard
    // stop everything
    drop(test_factory);

}
