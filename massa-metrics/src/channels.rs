use crossbeam::channel::{bounded, unbounded, Receiver, RecvError, SendError, Sender};
use prometheus::{Counter, Gauge};

#[derive(Clone)]
pub struct ChannelMetrics<T> {
    pub channel: (Sender<T>, Receiver<T>),
    name: String,
    actual_len: Gauge,
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
        let actual_len = Gauge::new(
            format!("{}_actual_size", name.clone()),
            "Actual length of channel",
        )
        .expect("Failed to create gauge");
        let received = Counter::new(
            format!("{}_total_receive", name.clone()),
            "Total received messages",
        )
        .expect("Failed to create counter");
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

    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        let res = self.channel.0.send(msg);
        self.actual_len.inc();
        res
    }

    fn len(&self) -> usize {
        self.channel.0.len()
    }

    pub fn recv(&self) -> Result<T, RecvError> {
        let res = self.channel.1.recv();
        self.actual_len.dec();
        self.received.inc();
        res
    }
}
