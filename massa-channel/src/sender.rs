use std::ops::Deref;

use crossbeam::channel::{SendError, Sender};
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
}

impl<T> Deref for MassaSender<T> {
    type Target = Sender<T>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}
