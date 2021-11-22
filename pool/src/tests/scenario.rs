// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::tools::get_transaction;
use crate::operation_pool::tests::POOL_SETTINGS;
use crate::tests::tools::{self, get_transaction_with_addresses, pool_test};
use crate::PoolSettings;
use models::Address;
use models::EndorsementHashMap;
use models::Operation;
use models::OperationHashMap;
use models::OperationHashSet;
use models::OperationId;
use models::SerializeCompact;
use models::Slot;
use protocol_exports::ProtocolCommand;
use serial_test::serial;
use signature::{derive_public_key, generate_random_private_key};
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
#[serial]
async fn test_pool() {
    let (cfg, thread_count, operation_validity_periods, max_pool_size_per_thread): &(
        PoolSettings,
        u8,
        u64,
        u64,
    ) = &POOL_SETTINGS;

    pool_test(
        &cfg,
        *thread_count,
        *operation_validity_periods,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
                _ => None,
            };
            // generate transactions
            let mut thread_tx_lists = vec![Vec::new(); *thread_count as usize];
            for i in 0..18 {
                let fee = 40 + i;
                let expire_period: u64 = 40 + i;
                let start_period = expire_period.saturating_sub(*operation_validity_periods);
                let (op, thread) = get_transaction(expire_period, fee);
                let id = op.verify_integrity().unwrap();

                let mut ops = OperationHashMap::default();
                ops.insert(id, op.clone());

                pool_command_sender
                    .add_operations(ops.clone())
                    .await
                    .unwrap();

                let newly_added = match protocol_controller
                    .wait_command(250.into(), op_filter.clone())
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

                match protocol_controller
                    .wait_command(250.into(), op_filter.clone())
                    .await
                {
                    Some(cmd) => panic!("unexpected protocol command {:?}", cmd),
                    None => {} // no propagation
                };

                thread_tx_lists[thread as usize].push((id, op, start_period..=expire_period));
            }
            // sort from bigger fee to smaller and truncate
            for lst in thread_tx_lists.iter_mut() {
                lst.reverse();
                lst.truncate(*max_pool_size_per_thread as usize);
            }

            // checks ops for thread 0 and 1 and various periods
            for thread in 0u8..=1 {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let max_count = 3;
                    let res = pool_command_sender
                        .get_operation_batch(
                            target_slot,
                            OperationHashSet::default(),
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
            // op ending before or at period 45 should be discarded
            let final_period = 45u64;
            pool_command_sender
                .update_latest_final_periods(vec![final_period; *thread_count as usize])
                .await
                .unwrap();
            for lst in thread_tx_lists.iter_mut() {
                lst.retain(|(_, op, _)| op.content.expire_period > final_period);
            }
            // checks ops for thread 0 and 1 and various periods
            for thread in 0u8..=1 {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let max_count = 4;
                    let res = pool_command_sender
                        .get_operation_batch(
                            target_slot,
                            OperationHashSet::default(),
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
            // add transactions from protocol with a high fee but too much in the future: should be ignored
            {
                pool_command_sender
                    .update_current_slot(Slot::new(10, 0))
                    .await
                    .unwrap();
                let fee = 1000;
                let expire_period: u64 = 300;
                let (op, thread) = get_transaction(expire_period, fee);
                let id = op.verify_integrity().unwrap();
                let mut ops = OperationHashMap::default();
                ops.insert(id, op);

                pool_command_sender.add_operations(ops).await.unwrap();

                match protocol_controller
                    .wait_command(250.into(), op_filter.clone())
                    .await
                {
                    Some(cmd) => panic!("unexpected protocol command {:?}", cmd),
                    None => {} // no propagation
                };
                let res = pool_command_sender
                    .get_operation_batch(
                        Slot::new(expire_period - 1, thread),
                        OperationHashSet::default(),
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
    let (cfg, thread_count, operation_validity_periods, _): &(PoolSettings, u8, u64, u64) =
        &POOL_SETTINGS;

    pool_test(
        &cfg,
        *thread_count,
        *operation_validity_periods,
        async move |mut protocol_controller, pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
                _ => None,
            };

            // generate transactions
            let mut thread_tx_lists = vec![Vec::new(); *thread_count as usize];
            for i in 0..18 {
                let fee = 40 + i;
                let expire_period: u64 = 40 + i;
                let start_period = expire_period.saturating_sub(*operation_validity_periods);
                let (op, thread) = get_transaction(expire_period, fee);
                let id = op.verify_integrity().unwrap();

                let mut ops = OperationHashMap::default();
                ops.insert(id, op.clone());

                protocol_controller.received_operations(ops.clone()).await;

                let newly_added = match protocol_controller
                    .wait_command(250.into(), op_filter.clone())
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

                match protocol_controller
                    .wait_command(250.into(), op_filter.clone())
                    .await
                {
                    Some(cmd) => panic!("unexpected protocol command {:?}", cmd),
                    None => {} // no propagation
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
    let (cfg, thread_count, operation_validity_periods, _): &(PoolSettings, u8, u64, u64) =
        &POOL_SETTINGS;

    pool_test(
        &cfg,
        *thread_count,
        *operation_validity_periods,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateEndorsements(_) => Some(cmd),
                _ => None,
            };
            let target_slot = Slot::new(10, 0);
            let endorsement = tools::create_endorsement(target_slot);
            let mut endorsements = EndorsementHashMap::default();
            let id = endorsement.compute_endorsement_id().unwrap();
            endorsements.insert(id.clone(), endorsement.clone());

            protocol_controller
                .received_endorsements(endorsements.clone())
                .await;

            let newly_added = match protocol_controller
                .wait_command(250.into(), op_filter.clone())
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

            match protocol_controller
                .wait_command(250.into(), op_filter.clone())
                .await
            {
                Some(cmd) => panic!("unexpected protocol command {:?}", cmd),
                None => {} // no propagation
            };

            let res = pool_command_sender
                .get_endorsements(
                    target_slot,
                    endorsement.content.endorsed_block,
                    vec![Address::from_public_key(&endorsement.content.sender_public_key).unwrap()],
                )
                .await
                .unwrap();
            assert_eq!(res.len(), 1);
            assert_eq!(res[0].0, endorsement.compute_endorsement_id().unwrap());
            (protocol_controller, pool_command_sender, pool_manager)
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_pool_add_old_endorsements() {
    let (cfg, thread_count, operation_validity_periods, _): &(PoolSettings, u8, u64, u64) =
        &POOL_SETTINGS;

    pool_test(
        &cfg,
        *thread_count,
        *operation_validity_periods,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateEndorsements(_) => Some(cmd),
                _ => None,
            };

            let endorsement = tools::create_endorsement(Slot::new(1, 0));
            let mut endorsements = EndorsementHashMap::default();
            let id = endorsement.compute_endorsement_id().unwrap();
            endorsements.insert(id.clone(), endorsement.clone());

            pool_command_sender
                .update_latest_final_periods(vec![50, 50])
                .await
                .unwrap();
            sleep(Duration::from_millis(500)).await;
            protocol_controller
                .received_endorsements(endorsements.clone())
                .await;

            match protocol_controller
                .wait_command(250.into(), op_filter.clone())
                .await
            {
                Some(cmd) => panic!("unexpected protocol command {:?}", cmd),
                None => {} // no propagation: endorsement is too old compared to final periods
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
    let mut address_a = Address::from_public_key(&pubkey_a).unwrap();
    while 1 != address_a.get_thread(thread_count) {
        priv_a = generate_random_private_key();
        pubkey_a = derive_public_key(&priv_a);
        address_a = Address::from_public_key(&pubkey_a).unwrap();
    }
    assert_eq!(1, address_a.get_thread(thread_count));

    let mut priv_b = generate_random_private_key();
    let mut pubkey_b = derive_public_key(&priv_b);
    let mut address_b = Address::from_public_key(&pubkey_b).unwrap();
    while 1 != address_b.get_thread(thread_count) {
        priv_b = generate_random_private_key();
        pubkey_b = derive_public_key(&priv_b);
        address_b = Address::from_public_key(&pubkey_b).unwrap();
    }
    assert_eq!(1, address_b.get_thread(thread_count));

    let (cfg, thread_count, operation_validity_periods, _): &(PoolSettings, u8, u64, u64) =
        &POOL_SETTINGS;

    pool_test(
        &cfg,
        *thread_count,
        *operation_validity_periods,
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
            let op1_id = op1.get_operation_id().unwrap();
            let op2_id = op2.get_operation_id().unwrap();
            let op3_id = op3.get_operation_id().unwrap();
            let mut ops = OperationHashMap::default();
            for (op, id) in vec![op1, op2, op3]
                .into_iter()
                .zip(vec![op1_id, op2_id, op3_id].into_iter())
            {
                ops.insert(id, op.clone());
            }

            // Add ops to pool
            protocol_controller.received_operations(ops.clone()).await;

            let newly_added = match protocol_controller
                .wait_command(250.into(), op_filter.clone())
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
    let mut address_a = Address::from_public_key(&pubkey_a).unwrap();
    while 0 != address_a.get_thread(thread_count) {
        priv_a = generate_random_private_key();
        pubkey_a = derive_public_key(&priv_a);
        address_a = Address::from_public_key(&pubkey_a).unwrap();
    }
    assert_eq!(0, address_a.get_thread(thread_count));

    let mut priv_b = generate_random_private_key();
    let mut pubkey_b = derive_public_key(&priv_b);
    let mut address_b = Address::from_public_key(&pubkey_b).unwrap();
    while 0 != address_b.get_thread(thread_count) {
        priv_b = generate_random_private_key();
        pubkey_b = derive_public_key(&priv_b);
        address_b = Address::from_public_key(&pubkey_b).unwrap();
    }
    assert_eq!(0, address_b.get_thread(thread_count));

    let (cfg, thread_count, operation_validity_periods, _): &(PoolSettings, u8, u64, u64) =
        &POOL_SETTINGS;

    pool_test(
        &cfg,
        *thread_count,
        *operation_validity_periods,
        async move |mut protocol_controller, mut pool_command_sender, pool_manager| {
            let op_filter = |cmd| match cmd {
                cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
                _ => None,
            };

            let mut ops: Vec<(OperationId, Operation)> = Vec::new();
            for i in 0..10 {
                let (op, _) = get_transaction_with_addresses(8, i, pubkey_a, priv_a, pubkey_b);
                ops.push((op.get_operation_id().unwrap(), op));
            }

            // Add ops to pool
            protocol_controller
                .received_operations(ops.clone().into_iter().collect::<OperationHashMap<_>>())
                .await;

            let newly_added = match protocol_controller
                .wait_command(250.into(), op_filter.clone())
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
                        .collect::<OperationHashMap<(u64, u8)>>(),
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
                        .collect::<OperationHashMap<_>>(),
                )
                .await;

            match protocol_controller
                .wait_command(500.into(), op_filter.clone())
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
