// use std::time::Duration;

use prometheus::{IntCounter, IntGauge};
#[allow(unused_imports)]
use tracing::warn;

pub struct MassaSurvey {}

impl MassaSurvey {
    #[allow(unused_variables)]
    pub fn run(
        tick_delay: std::time::Duration,
        active_in_connections: IntGauge,
        active_out_connections: IntGauge,
        peernet_total_bytes_sent: IntCounter,
        peernet_total_bytes_receive: IntCounter,
    ) {
        #[cfg(not(feature = "sandbox"))]
        {
            let mut data_sent = 0;
            let mut data_received = 0;
            std::thread::spawn(move || loop {
                std::thread::sleep(tick_delay);

                if active_in_connections.get() + active_out_connections.get() == 0 {
                    warn!("PEERNET | No active connections");
                }

                let new_data_sent = peernet_total_bytes_sent.get();
                let new_data_received = peernet_total_bytes_receive.get();

                if new_data_sent == data_sent && new_data_received == data_received {
                    warn!("PEERNET | No data sent or received since 5s");
                } else {
                    data_sent = new_data_sent;
                    data_received = new_data_received;
                }
            });
        }
    }
}
