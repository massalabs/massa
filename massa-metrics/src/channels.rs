use crossbeam::channel::{bounded, unbounded, Receiver, RecvError, SendError, Sender};
use prometheus::{Counter, Gauge};
use std::ops::{Deref, DerefMut};

#[derive(Clone)]
pub struct MassaChannel {}

#[derive(Clone)]
pub struct MassaSender<T> {
    sender: Sender<T>,
    name: String,
    actual_len: Gauge,
}

#[derive(Clone)]
pub struct MassaReceiver<T> {
    receiver: Receiver<T>,
    name: String,
    actual_len: Gauge,
    received: Counter,
}

impl MassaChannel {
    /// Create new channel with optional capacity.
    pub fn new<T>(name: String, capacity: Option<usize>) -> (MassaSender<T>, MassaReceiver<T>) {
        let (s, r) = if let Some(capacity) = capacity {
            bounded::<T>(capacity)
        } else {
            unbounded::<T>()
        };

        // Create gauge for actual length of channel
        // this can be inc() when sending msg or dec() when receive
        let actual_len = Gauge::new(
            format!("{}_channel_actual_size", name.clone()),
            "Actual length of channel",
        )
        .expect("Failed to create gauge");

        // Create counter for total received messages
        let received = Counter::new(
            format!("{}_channel_total_receive", name.clone()),
            "Total received messages",
        )
        .expect("Failed to create counter");

        let sender = MassaSender {
            sender: s,
            name: name.clone(),
            actual_len: actual_len.clone(),
        };

        let receiver = MassaReceiver {
            receiver: r,
            name,
            actual_len: actual_len.clone(),
            received: received.clone(),
        };

        // Register metrics in prometheus
        // TODO unwrap
        // panic here if metrics already registered (ex : ProtocolController>::get_stats )
        prometheus::register(Box::new(actual_len.clone())).unwrap();
        prometheus::register(Box::new(received.clone())).unwrap();

        (sender, receiver)
    }
}

impl<T> MassaSender<T> {
    /// Send a message to the channel
    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        let res = self.sender.send(msg);
        self.actual_len.inc();
        res
    }
}

impl<T> Deref for MassaSender<T> {
    type Target = Sender<T>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

impl<T> MassaReceiver<T> {
    pub fn recv(&self) -> Result<T, RecvError> {
        let res = self.receiver.recv();
        self.actual_len.dec();
        self.received.inc();
        res
    }
}

impl<T> Deref for MassaReceiver<T> {
    type Target = Receiver<T>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

impl<T> DerefMut for MassaReceiver<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.receiver
    }
}
