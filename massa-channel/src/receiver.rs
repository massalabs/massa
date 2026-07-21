use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam::channel::{Receiver, RecvError, RecvTimeoutError, TryRecvError};
use prometheus::{Counter, Gauge};
use tracing::trace;

#[derive(Clone)]
pub struct MassaReceiver<T> {
    pub(crate) receiver: Receiver<T>,
    #[allow(dead_code)]
    pub(crate) name: String,
    /// channel size
    pub(crate) actual_len: Gauge,
    /// total received messages
    pub(crate) received: Counter,
    /// reference counter to know how many receiver are cloned
    pub(crate) ref_counter: Arc<()>,
}

/// implement drop on MassaSender

impl<T> Drop for MassaReceiver<T> {
    fn drop(&mut self) {
        // this only unregisters if this is the last live clone
        self.unregister_metrics_if_last();
    }
}

impl<T> MassaReceiver<T> {
    /// increment manually the metrics
    /// Should be used when using the receiver with select! macro
    /// select! does not call recv()
    pub fn update_metrics(&self) {
        // use the len of the channel for actual_len instead of actual_len.dec()
        // because for each send we call recv more than one time
        self.actual_len.set(self.receiver.len() as f64);

        self.received.inc();
    }

    /// Unregister metrics only if this is the last live receiver clone.
    ///
    /// Channel disconnection is observed by *every* clone, so unregistering
    /// unconditionally from a `recv*`/`try_recv` disconnect branch would remove
    /// the metrics from the registry while sibling clones are still alive and
    /// updating them. Only the last remaining clone should unregister.
    fn unregister_metrics_if_last(&self) {
        if Arc::strong_count(&self.ref_counter) == 1 {
            self.unregister_metrics();
        }
    }

    /// unregister metrics
    fn unregister_metrics(&self) {
        if let Err(e) = prometheus::unregister(Box::new(self.actual_len.clone())) {
            trace!(
                "promethetus error unregister actual_len for {} : {}",
                self.name,
                e
            );
        }

        if let Err(e) = prometheus::unregister(Box::new(self.received.clone())) {
            trace!(
                "promethetus error unregister received for {} : {}",
                self.name,
                e
            );
        }
    }

    /// attempt to receive a message from the channel
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.receiver.try_recv() {
            Ok(msg) => {
                self.update_metrics();
                Ok(msg)
            }
            Err(crossbeam::channel::TryRecvError::Empty) => Err(TryRecvError::Empty),
            Err(crossbeam::channel::TryRecvError::Disconnected) => {
                self.unregister_metrics_if_last();
                Err(TryRecvError::Disconnected)
            }
        }
    }

    pub fn recv_deadline(&self, deadline: Instant) -> Result<T, RecvTimeoutError> {
        match self.receiver.recv_deadline(deadline) {
            Ok(msg) => {
                self.update_metrics();
                Ok(msg)
            }
            Err(RecvTimeoutError::Timeout) => Err(RecvTimeoutError::Timeout),
            Err(RecvTimeoutError::Disconnected) => {
                self.unregister_metrics_if_last();
                Err(RecvTimeoutError::Disconnected)
            }
        }
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(msg) => {
                self.update_metrics();
                Ok(msg)
            }
            Err(RecvTimeoutError::Timeout) => Err(RecvTimeoutError::Timeout),
            Err(RecvTimeoutError::Disconnected) => {
                self.unregister_metrics_if_last();
                Err(RecvTimeoutError::Disconnected)
            }
        }
    }

    pub fn recv(&self) -> Result<T, RecvError> {
        match self.receiver.recv() {
            Ok(msg) => {
                self.update_metrics();
                Ok(msg)
            }
            Err(e) => {
                self.unregister_metrics_if_last();
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

// Metrics are only registered in the default prometheus registry when the
// `test-exports` feature is disabled (see `MassaChannel::new`), so these
// registry-observing tests are gated to that build configuration.
#[cfg(all(test, not(feature = "test-exports")))]
mod tests {
    use crate::MassaChannel;

    /// Returns true if a metric family with the given name is currently
    /// registered in the default prometheus registry.
    fn metric_registered(name: &str) -> bool {
        prometheus::gather().iter().any(|mf| mf.get_name() == name)
    }

    #[test]
    fn disconnect_does_not_unregister_metrics_while_a_clone_is_alive() {
        let chan_name = "massa_channel_test_disconnect_keeps_metrics".to_string();
        let size_metric = format!("{}_channel_actual_size", chan_name);
        let recv_metric = format!("{}_channel_total_receive", chan_name);

        let (sender, receiver) = MassaChannel::new::<u8>(chan_name, None);
        // Second live clone of the receiver.
        let receiver2 = receiver.clone();

        assert!(metric_registered(&size_metric));
        assert!(metric_registered(&recv_metric));

        // Disconnect the channel; every receiver clone now observes disconnection.
        drop(sender);

        // Hitting the disconnect branch on one clone must NOT tear down the
        // metrics, because `receiver2` is still alive and using them.
        assert!(receiver.recv().is_err());
        assert!(
            metric_registered(&size_metric),
            "actual_size metric must stay registered while a clone is alive"
        );
        assert!(
            metric_registered(&recv_metric),
            "total_receive metric must stay registered while a clone is alive"
        );

        // Once the last clone is dropped, the metrics are unregistered.
        drop(receiver);
        drop(receiver2);
        assert!(
            !metric_registered(&size_metric),
            "actual_size metric must be unregistered after the last clone drops"
        );
        assert!(
            !metric_registered(&recv_metric),
            "total_receive metric must be unregistered after the last clone drops"
        );
    }
}
