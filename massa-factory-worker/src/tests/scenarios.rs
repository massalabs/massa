use std::str::FromStr;

use massa_models::{
    wrapped::WrappedContent, Amount, Operation, OperationSerializer, OperationType,
};
use massa_signature::KeyPair;

use super::TestFactory;

#[test]
#[serial_test::serial]
fn basic_creation() {
    let keypair = KeyPair::generate();
    let mut test_factory = TestFactory::new(&keypair);
    let (block_id, storage) = test_factory.get_next_created_block(None, None);
    assert_eq!(
        block_id,
        storage.retrieve_block(&block_id).unwrap().read().id
    );
}

#[test]
#[serial_test::serial]
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

    let block = storage.retrieve_block(&block_id).unwrap();
    for op_id in block.read().content.operations.iter() {
        storage.retrieve_operation(&op_id).unwrap();
    }
}
