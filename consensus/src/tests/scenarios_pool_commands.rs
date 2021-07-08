use std::collections::HashMap;

use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{start_consensus_controller, tests::tools::generate_ledger_file};
use models::Slot;
use pool::PoolCommand;
use serial_test::serial;
use time::UTime;

#[tokio::test]
#[serial]
async fn test_update_current_slot_cmd_notification() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let mut cfg = tools::default_consensus_config(1, ledger_file.path());
    cfg.t0 = 2000.into();
    cfg.genesis_timestamp = UTime::now(0).unwrap().checked_sub(100.into()).unwrap();

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (mut pool_controller, pool_command_sender) = MockPoolController::new();

    // launch consensus controller
    let (_consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let slot_notification_filter = |cmd| match cmd {
        pool::PoolCommand::UpdateCurrentSlot(slot) => Some(slot),
        _ => None,
    };

    //wait for UpdateCurrentSlot pool command
    let slot_cmd = pool_controller
        .wait_command(500.into(), slot_notification_filter)
        .await;
    assert_eq!(slot_cmd, Some(Slot::new(0, 0)));

    //wait for next UpdateCurrentSlot pool command
    let slot_cmd = pool_controller
        .wait_command(2000.into(), slot_notification_filter)
        .await;
    assert_eq!(slot_cmd, Some(Slot::new(0, 1)));

    // ignore all pool commands from now on
    let pool_sink = PoolCommandSink::new(pool_controller).await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    pool_sink.stop().await;
}

#[tokio::test]
#[serial]
async fn test_update_latest_final_block_cmd_notification() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let mut cfg = tools::default_consensus_config(1, ledger_file.path());
    cfg.t0 = 1000.into();
    cfg.genesis_timestamp = UTime::now(0).unwrap().checked_sub(100.into()).unwrap();
    cfg.delta_f0 = 2;
    cfg.disable_block_creation = false;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (mut pool_controller, pool_command_sender) = MockPoolController::new();

    // launch consensus controller
    let (_consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    // UpdateLatestFinalPeriods pool command filter
    let update_final_notification_filter = |cmd| match cmd {
        pool::PoolCommand::UpdateLatestFinalPeriods(periods) => Some(periods),
        PoolCommand::GetOperationBatch { response_tx, .. } => {
            response_tx.send(Vec::new()).unwrap();
            None
        }
        _ => None,
    };

    // wait for initial final periods notification
    let final_periods = pool_controller
        .wait_command(300.into(), update_final_notification_filter)
        .await;
    assert_eq!(final_periods, Some(vec![0, 0]));

    // wait for next final periods notification
    let final_periods = pool_controller
        .wait_command(
            (cfg.t0.to_millis() * 2 + 500).into(),
            update_final_notification_filter,
        )
        .await;
    assert_eq!(final_periods, Some(vec![1, 0]));

    // ignore all next pool commands
    let pool_sink = PoolCommandSink::new(pool_controller).await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
    pool_sink.stop().await;
}
