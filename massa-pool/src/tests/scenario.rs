// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::{settings::POOL_CONFIG, tools::get_transaction};
use crate::tests::tools::create_executesc;
use crate::tests::tools::{self, get_transaction_with_addresses, pool_test};
use massa_models::prehash::{Map, Set};
use massa_models::Address;
use massa_models::OperationId;
use massa_models::{Slot, WrappedOperation};
use massa_protocol_exports::ProtocolCommand;
use massa_signature::KeyPair;
use serial_test::serial;
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
#[serial]
async fn test_pool() {
    pool_test(
        &POOL_CONFIG,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
                _ => None,
            };
            // generate (id, transactions, range of validity) by threads
            let mut thread_tx_lists = vec![Vec::new(); POOL_CONFIG.thread_count as usize];
            for i in 0..18 {
                let fee = 40 + i;
                let expire_period: u64 = 40 + i;
                let start_period =
                    expire_period.saturating_sub(POOL_CONFIG.operation_validity_periods);
                let op = get_transaction(expire_period, fee);
                let id = op.verify_integrity().unwrap();

                let mut ops = Map::default();
                ops.insert(id, op.clone());

                pool_command_sender
                    .add_operations(ops.clone())
                    .await
                    .unwrap();

                let newly_added = match protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    Some(ProtocolCommand::PropagateOperations(ops)) => ops,
                    Some(_) => panic!("unexpected protocol command"),
                    None => panic!("unexpected timeout reached"),
                };
                assert_eq!(
                    newly_added.iter().copied().collect::<Vec<_>>(),
                    ops.keys().copied().collect::<Vec<_>>()
                );

                // duplicate
                pool_command_sender
                    .add_operations(ops.clone())
                    .await
                    .unwrap();

                if let Some(cmd) = protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    panic!("unexpected protocol command {:?}", cmd)
                };

                thread_tx_lists[op.thread as usize].push((op, start_period..=expire_period));
            }
            // sort from bigger fee to smaller and truncate
            for lst in thread_tx_lists.iter_mut() {
                lst.reverse();
                lst.truncate(POOL_CONFIG.settings.max_pool_size_per_thread as usize);
            }

            // checks ops are the expected ones for thread 0 and 1 and various periods
            for thread in 0u8..=1 {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let max_count = 3;
                    let res = pool_command_sender
                        .send_get_operations_announcement(
                            target_slot,
                            Set::<OperationId>::default(),
                            max_count,
                            10000,
                        )
                        .await
                        .unwrap();
                    assert!(res
                        .iter()
                        .map(|(op, _)| (op.id, op.serialized_data.clone()))
                        .eq(thread_tx_lists[target_slot.thread as usize]
                            .iter()
                            .filter(|(_, r)| r.contains(&target_slot.period))
                            .take(max_count)
                            .map(|(op, _)| (op.id, op.serialized_data.clone()))));
                }
            }
            // op ending before or at period 45 won't appear in the block due to incompatible validity range
            // we don't keep them as expected ops
            let final_period = 45u64;
            pool_command_sender
                .update_latest_final_periods(vec![final_period; POOL_CONFIG.thread_count as usize])
                .await
                .unwrap();
            for lst in thread_tx_lists.iter_mut() {
                lst.retain(|(op, _)| op.content.expire_period > final_period);
            }
            // checks ops are the expected ones for thread 0 and 1 and various periods
            for thread in 0u8..=1 {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let max_count = 4;
                    let res = pool_command_sender
                        .send_get_operations_announcement(
                            target_slot,
                            Set::<OperationId>::default(),
                            max_count,
                            10000,
                        )
                        .await
                        .unwrap();
                    assert!(res
                        .iter()
                        .map(|(op, _)| (op.id, op.serialized_data.clone()))
                        .eq(thread_tx_lists[target_slot.thread as usize]
                            .iter()
                            .filter(|(_, r)| r.contains(&target_slot.period))
                            .take(max_count)
                            .map(|(op, _)| (op.id, op.serialized_data.clone()))));
                }
            }
            // Add transactions that should be ignored despite their high fees, due to them being too far in the future
            {
                pool_command_sender
                    .update_current_slot(Slot::new(10, 0))
                    .await
                    .unwrap();
                let fee = 1000;
                let expire_period: u64 = 300;
                let op = get_transaction(expire_period, fee);
                let id = op.verify_integrity().unwrap();
                let mut ops = Map::default();
                ops.insert(id, op.clone());

                pool_command_sender.add_operations(ops).await.unwrap();

                if let Some(cmd) = protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    panic!("unexpected protocol command {:?}", cmd)
                };
                let res = pool_command_sender
                    .send_get_operations_announcement(
                        Slot::new(expire_period - 1, op.thread),
                        Set::<OperationId>::default(),
                        10,
                        10000,
                    )
                    .await
                    .unwrap();
                assert!(res.is_empty());
            }
            (protocol_controller, pool_command_sender, pool_manager)
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_pool_with_execute_sc() {
    pool_test(
        &POOL_CONFIG,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
                _ => None,
            };
            // generate (id, transactions, range of validity) by threads
            let mut thread_tx_lists = vec![Vec::new(); POOL_CONFIG.thread_count as usize];
            for i in 0..18 {
                let fee = 40 + i;
                let expire_period: u64 = 40 + i;
                let start_period =
                    expire_period.saturating_sub(POOL_CONFIG.operation_validity_periods);
                let op = create_executesc(expire_period, fee, 100, 1); // Only the fee determines the rentability
                let id = op.verify_integrity().unwrap();

                let mut ops = Map::default();
                ops.insert(id, op.clone());

                pool_command_sender
                    .add_operations(ops.clone())
                    .await
                    .unwrap();

                let newly_added = match protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    Some(ProtocolCommand::PropagateOperations(ops)) => ops,
                    Some(_) => panic!("unexpected protocol command"),
                    None => panic!("unexpected timeout reached"),
                };
                assert_eq!(
                    newly_added.iter().copied().collect::<Vec<_>>(),
                    ops.keys().copied().collect::<Vec<_>>()
                );

                // duplicate
                pool_command_sender
                    .add_operations(ops.clone())
                    .await
                    .unwrap();

                if let Some(cmd) = protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    panic!("unexpected protocol command {:?}", cmd)
                };

                thread_tx_lists[op.thread as usize].push((op, start_period..=expire_period));
            }
            // sort from bigger fee to smaller and truncate
            for lst in thread_tx_lists.iter_mut() {
                lst.reverse();
                lst.truncate(POOL_CONFIG.settings.max_pool_size_per_thread as usize);
            }

            // checks ops are the expected ones for thread 0 and 1 and various periods
            for thread in 0u8..=1 {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let max_count = 3;
                    let res = pool_command_sender
                        .send_get_operations_announcement(
                            target_slot,
                            Set::default(),
                            max_count,
                            10000,
                        )
                        .await
                        .unwrap();
                    assert!(res
                        .iter()
                        .map(|(op, _)| (op.id, op.serialized_data.clone()))
                        .eq(thread_tx_lists[target_slot.thread as usize]
                            .iter()
                            .filter(|(_, r)| r.contains(&target_slot.period))
                            .take(max_count)
                            .map(|(op, _)| (op.id, op.serialized_data.clone()))));
                }
            }
            // op ending before or at period 45 won't appear in the block due to incompatible validity range
            // we don't keep them as expected ops
            let final_period = 45u64;
            pool_command_sender
                .update_latest_final_periods(vec![final_period; POOL_CONFIG.thread_count as usize])
                .await
                .unwrap();
            for lst in thread_tx_lists.iter_mut() {
                lst.retain(|(op, _)| op.content.expire_period > final_period);
            }
            // checks ops are the expected ones for thread 0 and 1 and various periods
            for thread in 0u8..=1 {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let max_count = 4;
                    let res = pool_command_sender
                        .send_get_operations_announcement(
                            target_slot,
                            Set::default(),
                            max_count,
                            10000,
                        )
                        .await
                        .unwrap();
                    assert!(res
                        .iter()
                        .map(|(op, _)| (op.id, op.serialized_data.clone()))
                        .eq(thread_tx_lists[target_slot.thread as usize]
                            .iter()
                            .filter(|(_, r)| r.contains(&target_slot.period))
                            .take(max_count)
                            .map(|(op, _)| (op.id, op.serialized_data.clone()))));
                }
            }
            // Add transactions that should be ignored despite their high fees, due to them being too far in the future
            {
                pool_command_sender
                    .update_current_slot(Slot::new(10, 0))
                    .await
                    .unwrap();
                let fee = 1000;
                let expire_period: u64 = 300;
                let op = get_transaction(expire_period, fee);
                let id = op.verify_integrity().unwrap();
                let mut ops = Map::default();
                ops.insert(id, op.clone());

                pool_command_sender.add_operations(ops).await.unwrap();

                if let Some(cmd) = protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    panic!("unexpected protocol command {:?}", cmd)
                };
                let res = pool_command_sender
                    .send_get_operations_announcement(
                        Slot::new(expire_period - 1, op.thread),
                        Set::default(),
                        10,
                        10000,
                    )
                    .await
                    .unwrap();
                assert!(res.is_empty());
            }
            (protocol_controller, pool_command_sender, pool_manager)
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_pool_with_protocol_events() {
    pool_test(
        &POOL_CONFIG,
        async move |mut protocol_controller, pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
                _ => None,
            };

            // generate (id, transactions, range of validity) by threads
            let mut thread_tx_lists = vec![Vec::new(); POOL_CONFIG.thread_count as usize];
            for i in 0..18 {
                let fee = 40 + i;
                let expire_period: u64 = 40 + i;
                let start_period =
                    expire_period.saturating_sub(POOL_CONFIG.operation_validity_periods);
                let op = get_transaction(expire_period, fee);
                let id = op.verify_integrity().unwrap();

                let mut ops = Map::default();
                ops.insert(id, op.clone());

                protocol_controller.received_operations(ops.clone()).await;

                let newly_added = match protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    Some(ProtocolCommand::PropagateOperations(ops)) => ops,
                    Some(_) => panic!("unexpected protocol command"),
                    None => panic!("unexpected timeout reached"),
                };
                assert_eq!(
                    newly_added.iter().copied().collect::<Vec<_>>(),
                    ops.keys().copied().collect::<Vec<_>>()
                );

                // duplicate
                protocol_controller.received_operations(ops.clone()).await;

                if let Some(cmd) = protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    panic!("unexpected protocol command {:?}", cmd)
                };

                thread_tx_lists[op.thread as usize].push((id, op, start_period..=expire_period));
            }

            (protocol_controller, pool_command_sender, pool_manager)
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_pool_propagate_newly_added_endorsements() {
    pool_test(
        &POOL_CONFIG,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateEndorsements(_) => Some(cmd),
                _ => None,
            };
            let target_slot = Slot::new(10, 0);
            let endorsement = tools::create_endorsement(target_slot);
            let mut endorsements = Map::default();
            let id = endorsement.id;
            endorsements.insert(id, endorsement.clone());

            protocol_controller
                .received_endorsements(endorsements.clone())
                .await;

            let newly_added = match protocol_controller
                .wait_command(250.into(), op_filter)
                .await
            {
                Some(ProtocolCommand::PropagateEndorsements(endorsements)) => endorsements,
                Some(_) => panic!("unexpected protocol command"),
                None => panic!("unexpected timeout reached"),
            };
            assert!(newly_added.contains_key(&id));
            assert_eq!(newly_added.len(), 1);

            // duplicate
            protocol_controller
                .received_endorsements(endorsements)
                .await;

            if let Some(cmd) = protocol_controller
                .wait_command(250.into(), op_filter)
                .await
            {
                panic!("unexpected protocol command {:?}", cmd)
            };

            let res = pool_command_sender
                .get_endorsements(
                    target_slot,
                    endorsement.content.endorsed_block,
                    vec![endorsement.creator_address],
                )
                .await
                .unwrap();
            assert_eq!(res.len(), 1);
            assert_eq!(res[0].id, endorsement.id);
            (protocol_controller, pool_command_sender, pool_manager)
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_pool_add_old_endorsements() {
    pool_test(
        &POOL_CONFIG,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateEndorsements(_) => Some(cmd),
                _ => None,
            };

            let endorsement = tools::create_endorsement(Slot::new(1, 0));
            let mut endorsements = Map::default();
            let id = endorsement.id;
            endorsements.insert(id, endorsement.clone());

            pool_command_sender
                .update_latest_final_periods(vec![50, 50])
                .await
                .unwrap();
            sleep(Duration::from_millis(500)).await;
            protocol_controller
                .received_endorsements(endorsements.clone())
                .await;

            if let Some(cmd) = protocol_controller
                .wait_command(250.into(), op_filter)
                .await
            {
                panic!("unexpected protocol command {:?}", cmd)
            };

            (protocol_controller, pool_command_sender, pool_manager)
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_get_involved_operations() {
    let thread_count = 2;
    // define addresses use for the test
    // addresses a and b both in thread 0
    let mut keypair_a = KeyPair::generate();
    let mut address_a = Address::from_public_key(&keypair_a.get_public_key());
    while 1 != address_a.get_thread(thread_count) {
        keypair_a = KeyPair::generate();
        address_a = Address::from_public_key(&keypair_a.get_public_key());
    }
    assert_eq!(1, address_a.get_thread(thread_count));

    let mut keypair_b = KeyPair::generate();
    let mut address_b = Address::from_public_key(&keypair_b.get_public_key());
    while 1 != address_b.get_thread(thread_count) {
        keypair_b = KeyPair::generate();
        address_b = Address::from_public_key(&keypair_b.get_public_key());
    }
    assert_eq!(1, address_b.get_thread(thread_count));

    pool_test(
        &POOL_CONFIG,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
                _ => None,
            };

            pool_command_sender
                .update_current_slot(Slot::new(1, 0))
                .await
                .unwrap();
            let op1 = get_transaction_with_addresses(1, 1, &keypair_a, keypair_b.get_public_key());
            let op2 = get_transaction_with_addresses(2, 10, &keypair_b, keypair_b.get_public_key());
            let op3 =
                get_transaction_with_addresses(3, 100, &keypair_a, keypair_a.get_public_key());
            let op1_id = op1.id;
            let op2_id = op2.id;
            let op3_id = op3.id;
            let mut ops = Map::default();
            for (op, id) in vec![op1, op2, op3]
                .into_iter()
                .zip(vec![op1_id, op2_id, op3_id].into_iter())
            {
                ops.insert(id, op.clone());
            }

            // Add ops to pool
            protocol_controller.received_operations(ops.clone()).await;

            let newly_added = match protocol_controller
                .wait_command(250.into(), op_filter)
                .await
            {
                Some(ProtocolCommand::PropagateOperations(ops)) => ops,
                Some(_) => panic!("unexpected protocol command"),
                None => panic!("unexpected timeout reached"),
            };
            assert_eq!(
                newly_added.iter().copied().collect::<HashSet<_>>(),
                ops.keys().copied().collect::<HashSet<_>>()
            );

            let res = pool_command_sender
                .get_operations_involving_address(address_a)
                .await
                .unwrap();
            assert_eq!(
                res.keys().collect::<HashSet<_>>(),
                vec![&op1_id, &op3_id].into_iter().collect::<HashSet<_>>()
            );

            let res = pool_command_sender
                .get_operations_involving_address(address_b)
                .await
                .unwrap();
            assert_eq!(
                res.keys().collect::<HashSet<_>>(),
                vec![&op1_id, &op2_id].into_iter().collect::<HashSet<_>>()
            );

            pool_command_sender
                .update_latest_final_periods(vec![1, 1])
                .await
                .unwrap();

            let res = pool_command_sender
                .get_operations_involving_address(address_a)
                .await
                .unwrap();
            assert_eq!(
                res.keys().collect::<HashSet<_>>(),
                vec![&op3_id].into_iter().collect::<HashSet<_>>()
            );

            let res = pool_command_sender
                .get_operations_involving_address(address_b)
                .await
                .unwrap();
            assert_eq!(
                res.keys().collect::<HashSet<_>>(),
                vec![&op2_id].into_iter().collect::<HashSet<_>>()
            );

            pool_command_sender
                .update_latest_final_periods(vec![2, 2])
                .await
                .unwrap();

            let res = pool_command_sender
                .get_operations_involving_address(address_a)
                .await
                .unwrap();
            assert_eq!(
                res.keys().collect::<HashSet<_>>(),
                vec![&op3_id].into_iter().collect::<HashSet<_>>()
            );

            let res = pool_command_sender
                .get_operations_involving_address(address_b)
                .await
                .unwrap();
            assert!(res.is_empty());

            pool_command_sender
                .update_latest_final_periods(vec![3, 3])
                .await
                .unwrap();

            let res = pool_command_sender
                .get_operations_involving_address(address_a)
                .await
                .unwrap();
            assert!(res.is_empty());

            let res = pool_command_sender
                .get_operations_involving_address(address_b)
                .await
                .unwrap();
            assert!(res.is_empty());
            (protocol_controller, pool_command_sender, pool_manager)
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_new_final_ops() {
    let thread_count = 2;
    // define addresses use for the test
    // addresses a and b both in thread 0
    let mut keypair_a = KeyPair::generate();
    let mut address_a = Address::from_public_key(&keypair_a.get_public_key());
    while 0 != address_a.get_thread(thread_count) {
        keypair_a = KeyPair::generate();
        address_a = Address::from_public_key(&keypair_a.get_public_key());
    }
    assert_eq!(0, address_a.get_thread(thread_count));

    let mut keypair_b = KeyPair::generate();
    let mut address_b = Address::from_public_key(&keypair_b.get_public_key());
    while 0 != address_b.get_thread(thread_count) {
        keypair_b = KeyPair::generate();
        address_b = Address::from_public_key(&keypair_b.get_public_key());
    }
    assert_eq!(0, address_b.get_thread(thread_count));

    pool_test(
        &POOL_CONFIG,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
                _ => None,
            };

            let mut ops: Vec<WrappedOperation> = Vec::new();
            for i in 0..10 {
                let op =
                    get_transaction_with_addresses(8, i, &keypair_a, keypair_b.get_public_key());
                ops.push(op);
            }

            // Add ops to pool
            protocol_controller
                .received_operations(
                    ops.clone()
                        .into_iter()
                        .map(|op| (op.id, op.clone()))
                        .collect::<Map<OperationId, _>>(),
                )
                .await;

            let newly_added = match protocol_controller
                .wait_command(250.into(), op_filter)
                .await
            {
                Some(ProtocolCommand::PropagateOperations(ops)) => ops,
                Some(_) => panic!("unexpected protocol command"),
                None => panic!("unexpected timeout reached"),
            };
            assert_eq!(
                newly_added.iter().copied().collect::<HashSet<_>>(),
                ops.iter().map(|op| op.id).collect::<HashSet<_>>()
            );

            pool_command_sender
                .final_operations(
                    ops[..4]
                        .to_vec()
                        .iter()
                        .map(|op| (op.id, (8u64, 0u8)))
                        .collect::<Map<OperationId, (u64, u8)>>(),
                )
                .await
                .unwrap();

            let res = pool_command_sender
                .get_operations_involving_address(address_a)
                .await
                .unwrap();
            assert_eq!(
                res.keys().copied().collect::<HashSet<_>>(),
                ops[4..]
                    .to_vec()
                    .iter()
                    .map(|op| op.id)
                    .collect::<HashSet<_>>()
            );

            // try to add ops 0 to 3 to pool
            protocol_controller
                .received_operations(
                    ops[..4]
                        .to_vec()
                        .clone()
                        .into_iter()
                        .map(|op| (op.id, op))
                        .collect::<Map<OperationId, _>>(),
                )
                .await;

            match protocol_controller
                .wait_command(500.into(), op_filter)
                .await
            {
                Some(ProtocolCommand::PropagateOperations(_)) => {
                    panic!("unexpected operation propagation")
                }
                Some(_) => panic!("unexpected protocol command"),
                None => {}
            };
            (protocol_controller, pool_command_sender, pool_manager)
        },
    )
    .await;
}
