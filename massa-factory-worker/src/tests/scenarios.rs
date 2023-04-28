use super::TestFactory;
use massa_models::address::Address;
use massa_models::denunciation::{Denunciation, DenunciationPrecursor};
use massa_models::test_exports::{
    gen_block_headers_for_denunciation, gen_endorsements_for_denunciation,
};
use massa_models::{
    amount::Amount,
    operation::{Operation, OperationSerializer, OperationType},
    secure_share::SecureShareContent,
};
use massa_signature::KeyPair;
use std::str::FromStr;

/// Creates a basic empty block with the factory.
#[test]
#[ignore]
fn basic_creation() {
    let keypair = KeyPair::generate(0).unwrap();
    let mut test_factory = TestFactory::new(&keypair);
    let (block_id, storage) = test_factory.get_next_created_block(None, None);
    assert_eq!(block_id, storage.read_blocks().get(&block_id).unwrap().id);
}

/// Creates a block with a roll buy operation in it.
#[test]
#[ignore]
fn basic_creation_with_operation() {
    let keypair = KeyPair::generate(0).unwrap();
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
    let keypair = KeyPair::generate(0).unwrap();
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

/// Send some block headers and check if 1 (and only 1) Denunciation is produced
#[test]
fn test_denunciation_factory_block_header_denunciation() {
    let (_slot, keypair, secured_header_1, secured_header_2, secured_header_3) =
        gen_block_headers_for_denunciation();
    let address = Address::from_public_key(&keypair.get_public_key());

    // Built it to compare with what the factory will produce
    let denunciation_orig = Denunciation::try_from((&secured_header_1, &secured_header_2)).unwrap();
    // let _denunciation_id = DenunciationId::from(&_denunciation);

    let mut test_factory = TestFactory::new(&keypair);

    test_factory
        .denunciation_factory_sender
        .send(DenunciationPrecursor::try_from(&secured_header_1.clone()).unwrap())
        .unwrap();
    // Sending twice the same secured header - should not produce a Denunciation
    test_factory
        .denunciation_factory_sender
        .send(DenunciationPrecursor::try_from(&secured_header_1.clone()).unwrap())
        .unwrap();
    // With this one, it should produce a Denunciation
    test_factory
        .denunciation_factory_sender
        .send(DenunciationPrecursor::try_from(&secured_header_2.clone()).unwrap())
        .unwrap();
    // A denunciation has already been produced - expect this one will be ignored
    test_factory
        .denunciation_factory_sender
        .send(DenunciationPrecursor::try_from(&secured_header_3.clone()).unwrap())
        .unwrap();

    // Handle selector && pool responses - will timeout if nothing happens
    let from_pool = test_factory.denunciation_factory_loop(address);

    assert_eq!(from_pool.len(), 1);
    assert_eq!(from_pool[0], denunciation_orig);
    assert_eq!(
        from_pool[0]
            .is_valid()
            .expect("Denunciation should be a valid one"),
        true
    );
    assert_eq!(from_pool[0].is_for_block_header(), true);
    assert_eq!(
        from_pool[0]
            .is_also_for_block_header(&secured_header_3)
            .unwrap(),
        true
    );

    // stop everything
    drop(test_factory);
}

/// Send some block headers and check if 1 (and only 1) Denunciation is produced
#[test]
fn test_denunciation_factory_endorsement_denunciation() {
    let (_slot, keypair, s_endorsement_1, s_endorsement_2, s_endorsement_3) =
        gen_endorsements_for_denunciation();
    let address = Address::from_public_key(&keypair.get_public_key());

    let denunciation_orig: Denunciation = (&s_endorsement_1, &s_endorsement_2).try_into().unwrap();

    let mut test_factory = TestFactory::new(&keypair);

    test_factory
        .denunciation_factory_tx
        .send(DenunciationPrecursor::try_from(&s_endorsement_1.clone()).unwrap())
        .unwrap();
    // Sending twice the same secured header - should not produce a Denunciation
    test_factory
        .denunciation_factory_tx
        .send(DenunciationPrecursor::try_from(&s_endorsement_1.clone()).unwrap())
        .unwrap();
    // With this one, it should produce a Denunciation
    test_factory
        .denunciation_factory_tx
        .send(DenunciationPrecursor::try_from(&s_endorsement_2.clone()).unwrap())
        .unwrap();
    // A denunciation has already been produced - expect this one will be ignored
    test_factory
        .denunciation_factory_tx
        .send(DenunciationPrecursor::try_from(&s_endorsement_3.clone()).unwrap())
        .unwrap();

    // Handle selector && pool responses - will timeout if nothing happens
    let from_pool = test_factory.denunciation_factory_loop(address);

    assert_eq!(from_pool.len(), 1);
    assert_eq!(from_pool[0], denunciation_orig);
    assert_eq!(
        from_pool[0]
            .is_valid()
            .expect("Denunciation should be a valid one"),
        true
    );
    assert_eq!(from_pool[0].is_for_endorsement(), true);
    assert_eq!(
        from_pool[0]
            .is_also_for_endorsement(&s_endorsement_3)
            .unwrap(),
        true
    );

    // stop everything
    drop(test_factory);
}
