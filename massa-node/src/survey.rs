use std::thread::JoinHandle;

use massa_execution_exports::ExecutionController;
use massa_metrics::MassaMetrics;
use massa_models::{address::Address, slot::Slot, timeslots::get_latest_block_slot_at_timestamp};
use massa_time::MassaTime;
// use std::time::Duration;
#[allow(unused_imports)]
use tracing::warn;

pub struct MassaSurvey {}

impl MassaSurvey {
    #[allow(unused_variables)]
    // config : (thread_count, t0, genesis_timestamp, periods_per_cycle, last_start_period)
    pub fn run(
        tick_delay: std::time::Duration,
        execution_controller: Box<dyn ExecutionController>,
        massa_metrics: MassaMetrics,
        config: (u8, MassaTime, MassaTime, u64, u64),
    ) -> Option<JoinHandle<()>> {
        #[cfg(not(feature = "sandbox"))]
        {
            let mut data_sent = 0;
            let mut data_received = 0;
            match std::thread::Builder::new()
                .name("massa-survey".to_string())
                .spawn(move || loop {
                    std::thread::sleep(tick_delay);

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
                    }
                }) {
                Ok(handle) => Some(handle),
                Err(e) => {
                    warn!("MassaSurvey | Failed to spawn survey thread: {:?}", e);
                    None
                }
            }
        }

        #[cfg(feature = "sandbox")]
        {
            return None;
        }
    }
}
