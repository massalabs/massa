//! Massa Channel is a crossbeam channel wrapper with prometheus metrics
//! expose for each channel :
//! - actual length of channel (can be inc() when sending msg or dec() when receive)
//! - total received messages (inc() when receive)
//!
//! # Example
//! ```
//! use massa_channel::MassaChannel;
//! let (sender, receiver) = MassaChannel::new::<String>("test".to_string(), None);
//! ```
//!
//! # Warning
//! care about use MassaReceiver with select! macro
//! select! does not call recv() so metrics will not be updated
//! you should call `your_receiver.inc_metrics()` manually

use std::sync::Arc;

use receiver::MassaReceiver;
use sender::MassaSender;
use tracing::debug;

pub mod receiver;
pub mod sender;

#[derive(Clone)]
pub struct MassaChannel {}

impl MassaChannel {
    #[allow(clippy::new_ret_no_self)]
    pub fn new<T>(name: String, capacity: Option<usize>) -> (MassaSender<T>, MassaReceiver<T>) {
        use prometheus::{Counter, Gauge};

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
            debug!("Failed to register actual_len gauge for {} : {}", name, e);
        }

        if let Err(e) = prometheus::register(Box::new(received.clone())) {
            debug!("Failed to register received counter for {} : {}", name, e);
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
