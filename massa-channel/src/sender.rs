use std::{
    ops::Deref,
    time::{Duration, Instant},
};

use crossbeam::channel::{SendError, SendTimeoutError, Sender, TrySendError};
use prometheus::Gauge;

#[derive(Clone, Debug)]
pub struct MassaSender<T> {
    pub(crate) sender: Sender<T>,
    #[allow(dead_code)]
    pub(crate) name: String,
    /// channel size
    pub(crate) actual_len: Gauge,
}

impl<T> MassaSender<T> {
    /// Send a message to the channel
    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        match self.sender.send(msg) {
            Ok(()) => {
                self.actual_len.inc();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn send_timeout(&self, msg: T, duration: Duration) -> Result<(), SendTimeoutError<T>> {
        match self.sender.send_timeout(msg, duration) {
            Ok(()) => {
                self.actual_len.inc();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn send_deadline(&self, msg: T, deadline: Instant) -> Result<(), SendTimeoutError<T>> {
        match self.sender.send_deadline(msg, deadline) {
            Ok(()) => {
                self.actual_len.inc();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn try_send(&self, msg: T) -> Result<(), TrySendError<T>> {
        match self.sender.try_send(msg) {
            Ok(()) => {
                self.actual_len.inc();
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
