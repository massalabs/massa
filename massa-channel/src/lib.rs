use std::sync::Arc;

use receiver::MassaReceiver;
use sender::MassaSender;

pub mod receiver;
pub mod sender;

#[derive(Clone)]
pub struct MassaChannel {}

impl MassaChannel {
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T>(name: String, capacity: Option<usize>) -> (MassaSender<T>, MassaReceiver<T>) {
        use prometheus::{Counter, Gauge};
        use tracing::error;

        let (s, r) = if let Some(capacity) = capacity {
            crossbeam::channel::bounded::<T>(capacity)
        } else {
            crossbeam::channel::unbounded::<T>()
        };

        // Create gauge for actual length of channel
        // this can be inc() when sending msg or dec() when receive
        let actual_len = Gauge::new(
            format!("{}_channel_actual_size", name),
            "Actual length of channel",
        )
        .expect("Failed to create gauge");

        // Create counter for total received messages
        let received = Counter::new(
            format!("{}_channel_total_receive", name),
            "Total received messages",
        )
        .expect("Failed to create counter");

        // Register metrics in prometheus
        // error here if metrics already registered (ex : ProtocolController>::get_stats )
        if let Err(e) = prometheus::register(Box::new(actual_len.clone())) {
            error!("Failed to register actual_len gauge for {} : {}", name, e);
        }

        if let Err(e) = prometheus::register(Box::new(received.clone())) {
            error!("Failed to register received counter for {} : {}", name, e);
        }

        let sender = MassaSender {
            sender: s,
            name: name.clone(),
            actual_len: actual_len.clone(),
        };

        let receiver = MassaReceiver {
            receiver: r,
            name,
            actual_len,
            received,
            ref_counter: Arc::new(()),
        };

        (sender, receiver)
    }
}
