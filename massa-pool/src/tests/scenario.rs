// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::{settings::POOL_CONFIG, tools::get_transaction};
use crate::tests::tools::create_executesc;
use crate::tests::tools::{self, get_transaction_with_addresses, pool_test};
use massa_models::prehash::{Map, Set};
use massa_models::signed::Signable;
use massa_models::OperationId;
use massa_models::{Address, SerializeCompact};
use massa_models::{SignedOperation, Slot};
use massa_protocol_exports::ProtocolCommand;
use massa_signature::{derive_public_key, generate_random_private_key};
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
                let (op, thread) = get_transaction(expire_period, fee);
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
                    newly_added.keys().copied().collect::<Vec<_>>(),
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

                thread_tx_lists[thread as usize].push((id, op, start_period..=expire_period));
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
                        .get_operation_batch(
                            target_slot,
                            Set::<OperationId>::default(),
                            max_count,
                            10000,
                        )
                        .await
                        .unwrap();
                    assert!(res
                        .iter()
                        .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))
                        .eq(thread_tx_lists[target_slot.thread as usize]
                            .iter()
                            .filter(|(_, _, r)| r.contains(&target_slot.period))
                            .take(max_count)
                            .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))));
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
                lst.retain(|(_, op, _)| op.content.expire_period > final_period);
            }
            // checks ops are the expected ones for thread 0 and 1 and various periods
            for thread in 0u8..=1 {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let max_count = 4;
                    let res = pool_command_sender
                        .get_operation_batch(
                            target_slot,
                            Set::<OperationId>::default(),
                            max_count,
                            10000,
                        )
                        .await
                        .unwrap();
                    assert!(res
                        .iter()
                        .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))
                        .eq(thread_tx_lists[target_slot.thread as usize]
                            .iter()
                            .filter(|(_, _, r)| r.contains(&target_slot.period))
                            .take(max_count)
                            .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))));
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
                let (op, thread) = get_transaction(expire_period, fee);
                let id = op.verify_integrity().unwrap();
                let mut ops = Map::default();
                ops.insert(id, op);

                pool_command_sender.add_operations(ops).await.unwrap();

                if let Some(cmd) = protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    panic!("unexpected protocol command {:?}", cmd)
                };
                let res = pool_command_sender
                    .get_operation_batch(
                        Slot::new(expire_period - 1, thread),
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
                let (op, thread) = create_executesc(expire_period, fee, 100, 1); // Only the fee determines the rentability
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
                    newly_added.keys().copied().collect::<Vec<_>>(),
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

                thread_tx_lists[thread as usize].push((id, op, start_period..=expire_period));
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
                        .get_operation_batch(target_slot, Set::default(), max_count, 10000)
                        .await
                        .unwrap();
                    assert!(res
                        .iter()
                        .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))
                        .eq(thread_tx_lists[target_slot.thread as usize]
                            .iter()
                            .filter(|(_, _, r)| r.contains(&target_slot.period))
                            .take(max_count)
                            .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))));
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
                lst.retain(|(_, op, _)| op.content.expire_period > final_period);
            }
            // checks ops are the expected ones for thread 0 and 1 and various periods
            for thread in 0u8..=1 {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let max_count = 4;
                    let res = pool_command_sender
                        .get_operation_batch(target_slot, Set::default(), max_count, 10000)
                        .await
                        .unwrap();
                    assert!(res
                        .iter()
                        .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))
                        .eq(thread_tx_lists[target_slot.thread as usize]
                            .iter()
                            .filter(|(_, _, r)| r.contains(&target_slot.period))
                            .take(max_count)
                            .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))));
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
                let (op, thread) = get_transaction(expire_period, fee);
                let id = op.verify_integrity().unwrap();
                let mut ops = Map::default();
                ops.insert(id, op);

                pool_command_sender.add_operations(ops).await.unwrap();

                if let Some(cmd) = protocol_controller
                    .wait_command(250.into(), op_filter)
                    .await
                {
                    panic!("unexpected protocol command {:?}", cmd)
                };
                let res = pool_command_sender
                    .get_operation_batch(
                        Slot::new(expire_period - 1, thread),
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
                let (op, thread) = get_transaction(expire_period, fee);
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
                    newly_added.keys().copied().collect::<Vec<_>>(),
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

                thread_tx_lists[thread as usize].push((id, op, start_period..=expire_period));
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
            let id = endorsement.content.compute_id().unwrap();
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
                    vec![Address::from_public_key(
                        &endorsement.content.sender_public_key,
                    )],
                )
                .await
                .unwrap();
            assert_eq!(res.len(), 1);
            assert_eq!(res[0].0, endorsement.content.compute_id().unwrap());
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
            let id = endorsement.content.compute_id().unwrap();
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
    let mut priv_a = generate_random_private_key();
    let mut pubkey_a = derive_public_key(&priv_a);
    let mut address_a = Address::from_public_key(&pubkey_a);
    while 1 != address_a.get_thread(thread_count) {
        priv_a = generate_random_private_key();
        pubkey_a = derive_public_key(&priv_a);
        address_a = Address::from_public_key(&pubkey_a);
    }
    assert_eq!(1, address_a.get_thread(thread_count));

    let mut priv_b = generate_random_private_key();
    let mut pubkey_b = derive_public_key(&priv_b);
    let mut address_b = Address::from_public_key(&pubkey_b);
    while 1 != address_b.get_thread(thread_count) {
        priv_b = generate_random_private_key();
        pubkey_b = derive_public_key(&priv_b);
        address_b = Address::from_public_key(&pubkey_b);
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
            let (op1, _) = get_transaction_with_addresses(1, 1, pubkey_a, priv_a, pubkey_b);
            let (op2, _) = get_transaction_with_addresses(2, 10, pubkey_b, priv_b, pubkey_b);
            let (op3, _) = get_transaction_with_addresses(3, 100, pubkey_a, priv_a, pubkey_a);
            let op1_id = op1.content.compute_id().unwrap();
            let op2_id = op2.content.compute_id().unwrap();
            let op3_id = op3.content.compute_id().unwrap();
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
                newly_added.keys().copied().collect::<HashSet<_>>(),
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
    let mut priv_a = generate_random_private_key();
    let mut pubkey_a = derive_public_key(&priv_a);
    let mut address_a = Address::from_public_key(&pubkey_a);
    while 0 != address_a.get_thread(thread_count) {
        priv_a = generate_random_private_key();
        pubkey_a = derive_public_key(&priv_a);
        address_a = Address::from_public_key(&pubkey_a);
    }
    assert_eq!(0, address_a.get_thread(thread_count));

    let mut priv_b = generate_random_private_key();
    let mut pubkey_b = derive_public_key(&priv_b);
    let mut address_b = Address::from_public_key(&pubkey_b);
    while 0 != address_b.get_thread(thread_count) {
        priv_b = generate_random_private_key();
        pubkey_b = derive_public_key(&priv_b);
        address_b = Address::from_public_key(&pubkey_b);
    }
    assert_eq!(0, address_b.get_thread(thread_count));

    pool_test(
        &POOL_CONFIG,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
                _ => None,
            };

            let mut ops: Vec<(OperationId, SignedOperation)> = Vec::new();
            for i in 0..10 {
                let (op, _) = get_transaction_with_addresses(8, i, pubkey_a, priv_a, pubkey_b);
                ops.push((op.content.compute_id().unwrap(), op));
            }

            // Add ops to pool
            protocol_controller
                .received_operations(ops.clone().into_iter().collect::<Map<OperationId, _>>())
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
                newly_added.keys().copied().collect::<HashSet<_>>(),
                ops.iter().map(|(id, _)| *id).collect::<HashSet<_>>()
            );

            pool_command_sender
                .final_operations(
                    ops[..4]
                        .to_vec()
                        .iter()
                        .map(|(id, _)| (*id, (8u64, 0u8)))
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
                    .map(|(id, _)| *id)
                    .collect::<HashSet<_>>()
            );

            // try to add ops 0 to 3 to pool
            protocol_controller
                .received_operations(
                    ops[..4]
                        .to_vec()
                        .clone()
                        .into_iter()
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
