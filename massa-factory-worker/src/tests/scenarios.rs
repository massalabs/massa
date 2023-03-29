use super::TestFactory;
use massa_hash::Hash;
use massa_models::block_header::{BlockHeader, BlockHeaderSerializer, SecuredHeader};
use massa_models::block_id::BlockId;
use massa_models::config::{T0, THREAD_COUNT};
use massa_models::denunciation::{Denunciation, DenunciationId};
use massa_models::endorsement::{Endorsement, EndorsementSerializerLW};
use massa_models::slot::Slot;
use massa_models::timeslots::get_closest_slot_to_timestamp;
use massa_models::{
    amount::Amount,
    operation::{Operation, OperationSerializer, OperationType},
    secure_share::SecureShareContent,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;
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
    let operation =
        Operation::new_verifiable(content, OperationSerializer::new(), &keypair).unwrap();
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
    let operation =
        Operation::new_verifiable(content, OperationSerializer::new(), &keypair).unwrap();
    let (block_id, storage) =
        test_factory.get_next_created_block(Some(vec![operation.clone(), operation]), None);

    let block = storage.read_blocks().get(&block_id).unwrap().clone();
    for op_id in block.content.operations.iter() {
        storage.read_operations().get(op_id).unwrap();
    }
    assert_eq!(block.content.operations.len(), 2);
}

/// Send 2 block headers and check if a Denunciation op is in storage
#[test]
fn test_denunciation_factory_block_header_denunciation() {
    let keypair = KeyPair::generate();

    let now = MassaTime::now().expect("could not get current time");
    // get closest slot according to the current absolute time
    let slot = get_closest_slot_to_timestamp(THREAD_COUNT, T0, now, now);

    let parents: Vec<BlockId> = (0..THREAD_COUNT)
        .map(|i| BlockId(Hash::compute_from(&[i])))
        .collect();

    let parents2: Vec<BlockId> = (0..THREAD_COUNT)
        .map(|i| BlockId(Hash::compute_from(&[i + 1])))
        .collect();

    let header1 = BlockHeader {
        slot,
        parents: parents.clone(),
        operation_merkle_root: Hash::compute_from("mno".as_bytes()),
        endorsements: vec![Endorsement::new_verifiable(
            Endorsement {
                slot: Slot::new(1, 1),
                index: 1,
                endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
            },
            EndorsementSerializerLW::new(),
            &keypair,
        )
        .unwrap()],
    };

    let secured_header_1: SecuredHeader =
        BlockHeader::new_verifiable(header1, BlockHeaderSerializer::new(), &keypair).unwrap();

    let header2 = BlockHeader {
        slot,
        parents: parents2,
        operation_merkle_root: Hash::compute_from("mno".as_bytes()),
        endorsements: vec![Endorsement::new_verifiable(
            Endorsement {
                slot: Slot::new(1, 1),
                index: 1,
                endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
            },
            EndorsementSerializerLW::new(),
            &keypair,
        )
        .unwrap()],
    };

    let secured_header_2: SecuredHeader =
        BlockHeader::new_verifiable(header2, BlockHeaderSerializer::new(), &keypair).unwrap();

    // Built it to compare with what the factory will produce
    let denunciation = Denunciation::try_from((&secured_header_1, &secured_header_2)).unwrap();
    let denunciation_id = DenunciationId::from(&denunciation);

    let test_factory = TestFactory::new(&keypair);

    test_factory
        .denunciation_factory_sender
        .send(secured_header_1.clone())
        .unwrap();
    test_factory
        .denunciation_factory_sender
        .send(secured_header_2.clone())
        .unwrap();

    // Wait for denunciation factory to create the Denunciation
    std::thread::sleep(Duration::from_secs(1));

    let de_indexes = test_factory.storage.read_denunciations();
    assert_eq!(de_indexes.get(&denunciation_id), Some(&denunciation));

    // release RwLockReadGuard
    drop(de_indexes);
    // stop everything
    drop(test_factory);
}
