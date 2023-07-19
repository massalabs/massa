use std::thread::JoinHandle;

use massa_execution_exports::ExecutionController;
use massa_metrics::MassaMetrics;
// use std::time::Duration;
#[allow(unused_imports)]
use tracing::warn;

pub struct MassaSurvey {}

impl MassaSurvey {
    #[allow(unused_variables)]
    pub fn run(
        tick_delay: std::time::Duration,
        execution_controller: Box<dyn ExecutionController>,
        massa_metrics: MassaMetrics,
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
