#![allow(unused_imports)]
use std::thread::JoinHandle;

use crossbeam_channel::{select, tick};
use massa_channel::{sender::MassaSender, MassaChannel};
use massa_execution_exports::ExecutionController;
use massa_metrics::MassaMetrics;
use massa_models::{address::Address, slot::Slot, timeslots::get_latest_block_slot_at_timestamp};
use massa_pool_exports::PoolController;
use massa_time::MassaTime;
use tracing::info;
// use std::time::Duration;
use tracing::warn;

pub struct MassaSurvey {}

pub struct MassaSurveyStopper {
    tx_stopper: Option<MassaSender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl MassaSurveyStopper {
    pub fn stop(&mut self) {
        if let Some(tx) = self.tx_stopper.take() {
            info!("MassaSurvey | Stopping");
            if let Err(e) = tx.send(()) {
                warn!("failed to send stop signal to massa survey thread: {:?}", e);
            }
        }
        if let Some(handle) = self.handle.take() {
            match handle.join() {
                Ok(_) => info!("MassaSurvey | Stopped"),
                Err(_) => warn!("failed to join massa survey thread"),
            }
        }
    }
}

impl MassaSurvey {
    #[allow(unused_variables)]
    // config : (thread_count, t0, genesis_timestamp, periods_per_cycle, last_start_period)
    pub fn run(
        tick_delay: std::time::Duration,
        execution_controller: Box<dyn ExecutionController>,
        pool_controller: Box<dyn PoolController>,
        massa_metrics: MassaMetrics,
        config: (u8, MassaTime, MassaTime, u64, u64),
    ) -> MassaSurveyStopper {
        if massa_metrics.is_enabled() {
            #[cfg(not(feature = "sandbox"))]
            {
                let mut data_sent = 0;
                let mut data_received = 0;
                let (tx_stop, rx_stop) =
                    MassaChannel::new("massa_survey_stop".to_string(), Some(1));
                let update_tick = tick(tick_delay);
                match std::thread::Builder::new()
                    .name("massa-survey".to_string())
                    .spawn(move || loop {
                        select! {
                            recv(rx_stop) -> _ => {
                                break;
                            },
                            recv(update_tick) -> _ => {
                                let (
                                    active_in_connections,
                                    active_out_connections,
                                    new_data_sent,
                                    new_data_received,
                                ) = massa_metrics.get_metrics_for_survey_thread();

                                if active_in_connections + active_out_connections == 0 {
                                    warn!("PEERNET | No active connections");
                                }

                                if new_data_sent == data_sent && new_data_received == data_received {
                                    warn!("PEERNET | No data sent or received since 5s");
                                } else {
                                    data_sent = new_data_sent;
                                    data_received = new_data_received;
                                }

                                {
                                           // update stakers / rolls
                                    let now = match MassaTime::now() {
                                        Ok(now) => now,
                                        Err(e) => {
                                            warn!("MassaSurvey | Failed to get current time: {:?}", e);
                                            continue;
                                        }
                                    };

                                    let curr_cycle =
                                        match get_latest_block_slot_at_timestamp(config.0, config.1, config.2, now)
                                        {
                                            Ok(Some(cur_slot)) if cur_slot.period <= config.4 => {
                                                Slot::new(config.4, 0).get_cycle(config.3)
                                            }
                                            Ok(Some(cur_slot)) => cur_slot.get_cycle(config.3),
                                            Ok(None) => 0,
                                            Err(e) => {
                                                warn!(
                                                "MassaSurvey | Failed to get latest block slot at timestamp: {:?}",
                                                e
                                            );
                                                continue;
                                            }
                                        };

                                    let staker_vec = execution_controller
                                        .get_cycle_active_rolls(curr_cycle)
                                        .into_iter()
                                        .collect::<Vec<(Address, u64)>>();

                                    massa_metrics.set_stakers(staker_vec.len());
                                    let rolls_count = staker_vec.iter().map(|(_, r)| *r).sum::<u64>();
                                    massa_metrics.set_rolls(rolls_count as usize);
                                    let current_slot = get_latest_block_slot_at_timestamp(config.0, config.1, config.2, now).unwrap_or(None).unwrap_or(Slot::new(0, 0));
                                    massa_metrics.set_current_time_thread(current_slot.thread);
                                    massa_metrics.set_current_time_period(current_slot.period);
                                }

                                {
                                    massa_metrics.set_operations_pool(pool_controller.get_operation_count());
                                    massa_metrics.set_endorsements_pool(pool_controller.get_endorsement_count());
                                    massa_metrics.set_denunciations_pool(pool_controller.get_denunciation_count());

                                    let count = std::thread::available_parallelism()
                                    .unwrap_or(std::num::NonZeroUsize::MIN)
                                    .get();
                                    massa_metrics.set_available_processors(count);
                                }
                            }
                        }
                    }) {
                    Ok(handle) => MassaSurveyStopper { handle: Some(handle), tx_stopper: Some(tx_stop) },
                    Err(e) => {
                        warn!("MassaSurvey | Failed to spawn survey thread: {:?}", e);
                        MassaSurveyStopper { handle: None, tx_stopper: None}
                    }
                }
            }

            #[cfg(feature = "sandbox")]
            {
                MassaSurveyStopper {
                    handle: None,
                    tx_stopper: None,
                }
            }
        } else {
            MassaSurveyStopper {
                handle: None,
                tx_stopper: None,
            }
        }
    }
}
