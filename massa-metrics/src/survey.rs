use std::time::Duration;

use prometheus::IntGauge;
#[allow(unused_imports)]
use tracing::warn;

pub struct MassaSurvey {}

impl MassaSurvey {
    #[allow(unused_variables)]
    pub fn run(active_in_connections: IntGauge, active_out_connections: IntGauge) {
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));

            #[cfg(not(feature = "sandbox"))]
            {
                if active_in_connections.get() + active_out_connections.get() == 0 {
                    warn!("No active connections");
                }
            }
        });
    }
}
