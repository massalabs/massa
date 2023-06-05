use crossbeam::channel::{
    bounded, unbounded, Receiver, RecvError, SendError, Sender, TryRecvError,
};
use prometheus::{Counter, Gauge};
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};
use tracing::error;

#[derive(Clone)]
pub struct MassaChannel {}

#[derive(Clone)]
pub struct MassaSender<T> {
    sender: Sender<T>,
    name: String,
    /// channel size, none if metrics feature is not enabled
    actual_len: Option<Gauge>,
}

#[derive(Clone)]
pub struct MassaReceiver<T> {
    receiver: Receiver<T>,
    name: String,
    /// channel size, none if metrics feature is not enabled
    actual_len: Option<Gauge>,
    /// total received messages, none if metrics feature is not enabled
    received: Option<Counter>,
    /// reference counter to know how many receiver are cloned
    ref_counter: Arc<()>,
}

impl MassaChannel {
    /// Create new channel with optional capacity.
    pub fn new<T>(name: String, capacity: Option<usize>) -> (MassaSender<T>, MassaReceiver<T>) {
        let (s, r) = if let Some(capacity) = capacity {
            bounded::<T>(capacity)
        } else {
            unbounded::<T>()
        };

        let (actual_len, received) = if cfg!(feature = "metrics") {
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

            // Register metrics in prometheus
            // error here if metrics already registered (ex : ProtocolController>::get_stats )
            if let Err(e) = prometheus::register(Box::new(actual_len.clone())) {
                error!("Failed to register actual_len gauge: {}", e);
            }

            if let Err(e) = prometheus::register(Box::new(received.clone())) {
                error!("Failed to register received counter: {}", e);
            }

            (Some(actual_len), Some(received))
        } else {
            (None, None)
        };

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
            ref_counter: Arc::new(()),
        };

        (sender, receiver)
    }
}

impl<T> MassaSender<T> {
    /// Send a message to the channel
    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        match self.sender.send(msg) {
            Ok(()) => {
                if let Some(actual_len) = &self.actual_len {
                    actual_len.inc();
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

impl<T> Deref for MassaSender<T> {
    type Target = Sender<T>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

/// implement drop on MassaSender
impl<T> Drop for MassaReceiver<T> {
    fn drop(&mut self) {
        // info!("MassaReceiver dropped {}", &self.name);
        let ref_count = Arc::strong_count(&self.ref_counter);
        if ref_count == 1 {
            // this is the last ref so we can unregister metrics
            if let Some(actual_len) = &self.actual_len {
                let _ = prometheus::unregister(Box::new(actual_len.clone()));
                self.actual_len = None;
            }

            if let Some(received) = &self.received {
                let _ = prometheus::unregister(Box::new(received.clone()));
                self.received = None;
            }
        }
    }
}

impl<T> MassaReceiver<T> {
    /// attempt to receive a message from the channel
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.receiver.try_recv() {
            Ok(msg) => {
                if let Some(actual_len) = &self.actual_len {
                    // use the len of the channel for actual_len instead of actual_len.dec()
                    // because for each send we call recv more than one time
                    actual_len.set(self.receiver.len() as f64);
                }

                if let Some(received) = self.received.as_ref() {
                    received.inc();
                }

                Ok(msg)
            }
            Err(crossbeam::channel::TryRecvError::Empty) => Err(TryRecvError::Empty),
            Err(crossbeam::channel::TryRecvError::Disconnected) => {
                if let Some(actual_len) = &self.actual_len {
                    let _ = prometheus::unregister(Box::new(actual_len.clone()));
                }

                if let Some(received) = &self.received {
                    let _ = prometheus::unregister(Box::new(received.clone()));
                }
                Err(TryRecvError::Disconnected)
            }
        }
    }

    pub fn recv(&self) -> Result<T, RecvError> {
        match self.receiver.recv() {
            Ok(msg) => {
                if let Some(actual_len) = &self.actual_len {
                    // use the len of the channel for actual_len instead of actual_len.dec()
                    // because for each send we call recv more than one time
                    actual_len.set(self.receiver.len() as f64);
                }

                if let Some(received) = self.received.as_ref() {
                    received.inc();
                }

                Ok(msg)
            }
            Err(e) => {
                if let Some(actual_len) = &self.actual_len {
                    let _ = prometheus::unregister(Box::new(actual_len.clone()));
                }

                if let Some(received) = self.received.as_ref() {
                    match prometheus::unregister(Box::new(received.clone())) {
                        Ok(_) => {}
                        Err(e) => {
                            dbg!(e);
                        }
                    }
                }
                Err(e)
            }
        }
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
