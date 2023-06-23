use std::{
    ops::Deref,
    time::{Duration, Instant},
};

use crossbeam::channel::{SendError, SendTimeoutError, Sender, TrySendError};
use prometheus::Gauge;

#[derive(Debug, Clone)]
pub struct MassaSender<T> {
    pub sender: Sender<T>,
    #[allow(dead_code)]
    pub name: String,
    /// channel size
    pub actual_len: Gauge,
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
