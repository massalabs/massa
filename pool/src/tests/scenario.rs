use communication::protocol::ProtocolCommand;
use models::SerializeCompact;
use models::Slot;
use std::collections::HashMap;
use std::collections::HashSet;
use tokio::sync::oneshot;

use crate::pool_controller;

use super::{
    mock_protocol_controller::MockProtocolController,
    tools::{example_pool_config, get_transaction},
};

#[tokio::test]
async fn test_pool() {
    let (mut cfg, context, thread_count, operation_validity_periods) = example_pool_config();

    let max_pool_size_per_thread = 10;
    cfg.max_pool_size_per_thread = max_pool_size_per_thread;

    let (mut protocol_controller, protocol_command_sender, protocol_pool_event_receiver) =
        MockProtocolController::new();

    let (mut pool_command_sender, pool_manager) = pool_controller::start_pool_controller(
        cfg.clone(),
        thread_count,
        operation_validity_periods,
        protocol_command_sender,
        protocol_pool_event_receiver,
        context.clone(),
    )
    .await
    .unwrap();
    let op_filter = |cmd| match cmd {
        cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
        _ => None,
    };

    // generate transactions
    let mut thread_tx_lists = vec![Vec::new(); thread_count as usize];
    for i in 0..18 {
        let fee = 40 + i;
        let expire_period: u64 = 40 + i;
        let start_period = expire_period.saturating_sub(operation_validity_periods);
        let (op, thread) = get_transaction(expire_period, fee, &context);
        let id = op.verify_integrity(&context).unwrap();

        let mut ops = HashMap::new();
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

        match protocol_controller
            .wait_command(250.into(), op_filter)
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
        lst.truncate(max_pool_size_per_thread as usize);
    }

    // checks ops for thread 0 and 1 and various periods
    for thread in 0u8..=1 {
        for period in 0u64..70 {
            let target_slot = Slot::new(period, thread);
            let max_count = 3;
            let (response_tx, response_rx) = oneshot::channel();
            pool_command_sender
                .get_operation_batch(target_slot, HashSet::new(), max_count, response_tx)
                .await
                .unwrap();
            let res = response_rx.await.unwrap();
            assert!(res
                .iter()
                .map(|(id, op)| (id, op.to_bytes_compact(&context).unwrap()))
                .eq(thread_tx_lists[target_slot.thread as usize]
                    .iter()
                    .filter(|(_, _, r)| r.contains(&target_slot.period))
                    .take(max_count)
                    .map(|(id, op, _)| (id, op.to_bytes_compact(&context).unwrap()))));
        }
    }

    // op ending before or at period 45 should be discarded
    let final_period = 45u64;
    pool_command_sender
        .update_latest_final_periods(vec![final_period; thread_count as usize])
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
            let (response_tx, response_rx) = oneshot::channel();
            pool_command_sender
                .get_operation_batch(target_slot, HashSet::new(), max_count, response_tx)
                .await
                .unwrap();
            let res = response_rx.await.unwrap();
            assert!(res
                .iter()
                .map(|(id, op)| (id, op.to_bytes_compact(&context).unwrap()))
                .eq(thread_tx_lists[target_slot.thread as usize]
                    .iter()
                    .filter(|(_, _, r)| r.contains(&target_slot.period))
                    .take(max_count)
                    .map(|(id, op, _)| (id, op.to_bytes_compact(&context).unwrap()))));
        }
    }

    // add transactions from protocolwith a high fee but too much in the future: should be ignored
    {
        pool_command_sender
            .update_current_slot(Slot::new(10, 0))
            .await
            .unwrap();
        let fee = 1000;
        let expire_period: u64 = 300;
        let (op, thread) = get_transaction(expire_period, fee, &context);
        let id = op.verify_integrity(&context).unwrap();
        let mut ops = HashMap::new();
        ops.insert(id, op);

        pool_command_sender.add_operations(ops).await.unwrap();

        match protocol_controller
            .wait_command(250.into(), op_filter)
            .await
        {
            Some(cmd) => panic!("unexpected protocol command {:?}", cmd),
            None => {} // no propagation
        };
        let (response_tx, response_rx) = oneshot::channel();
        pool_command_sender
            .get_operation_batch(
                Slot::new(expire_period - 1, thread),
                HashSet::new(),
                10,
                response_tx,
            )
            .await
            .unwrap();
        let res = response_rx.await.unwrap();
        assert!(res.is_empty());
    }

    pool_manager.stop().await.unwrap();
}
#[tokio::test]
async fn test_pool_with_protocol_events() {
    let (mut cfg, context, thread_count, operation_validity_periods) = example_pool_config();

    let max_pool_size_per_thread = 10;
    cfg.max_pool_size_per_thread = max_pool_size_per_thread;

    let (mut protocol_controller, protocol_command_sender, protocol_pool_event_receiver) =
        MockProtocolController::new();

    let (_pool_command_sender, pool_manager) = pool_controller::start_pool_controller(
        cfg.clone(),
        thread_count,
        operation_validity_periods,
        protocol_command_sender,
        protocol_pool_event_receiver,
        context.clone(),
    )
    .await
    .unwrap();
    let op_filter = |cmd| match cmd {
        cmd @ ProtocolCommand::PropagateOperations(_) => Some(cmd),
        _ => None,
    };

    // generate transactions
    let mut thread_tx_lists = vec![Vec::new(); thread_count as usize];
    for i in 0..18 {
        let fee = 40 + i;
        let expire_period: u64 = 40 + i;
        let start_period = expire_period.saturating_sub(operation_validity_periods);
        let (op, thread) = get_transaction(expire_period, fee, &context);
        let id = op.verify_integrity(&context).unwrap();

        let mut ops = HashMap::new();
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

        match protocol_controller
            .wait_command(250.into(), op_filter)
            .await
        {
            Some(cmd) => panic!("unexpected protocol command {:?}", cmd),
            None => {} // no propagation
        };

        thread_tx_lists[thread as usize].push((id, op, start_period..=expire_period));
    }

    pool_manager.stop().await.unwrap();
}
