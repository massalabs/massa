use crossbeam::channel::{bounded, unbounded, Receiver, RecvError, SendError, Sender};
use prometheus::{Counter, Gauge};
use std::ops::{Deref, DerefMut};

#[derive(Clone)]
pub struct ChannelMetrics<T> {
    channel: (Sender<T>, Receiver<T>),
    /// Channel name
    name: String,
    /// gauge for actual length of channel
    actual_len: Gauge,
    /// counter for total received messages
    received: Counter,
}

impl<T> ChannelMetrics<T> {
    /// Create new channel with optional capacity.
    pub fn new(name: String, capacity: Option<usize>) -> Self {
        let (s, r) = if let Some(capacity) = capacity {
            bounded::<T>(capacity)
        } else {
            unbounded::<T>()
        };

        // Create gauge for actual length of channel
        // this can be inc() when sending msg or dec() when receive
        let actual_len = Gauge::new(
            format!("{}_actual_size", name.clone()),
            "Actual length of channel",
        )
        .expect("Failed to create gauge");

        // Create counter for total received messages
        let received = Counter::new(
            format!("{}_total_receive", name.clone()),
            "Total received messages",
        )
        .expect("Failed to create counter");

        // Register metrics in prometheus
        // TODO unwrap
        prometheus::register(Box::new(actual_len.clone())).unwrap();
        prometheus::register(Box::new(received.clone())).unwrap();
        Self {
            channel: (s, r),
            actual_len,
            received,
            name,
        }
    }

    /// Send a message to the channel
    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        let res = self.channel.0.send(msg);
        self.actual_len.inc();
        res
    }

    pub fn recv(&self) -> Result<T, RecvError> {
        let res = self.channel.1.recv();
        self.actual_len.dec();
        self.received.inc();
        res
    }
}

impl<T> Deref for ChannelMetrics<T> {
    type Target = (Sender<T>, Receiver<T>);

    fn deref(&self) -> &Self::Target {
        &self.channel
    }
}

impl<T> DerefMut for ChannelMetrics<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.channel
    }
}
