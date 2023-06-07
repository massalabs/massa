use std::ops::Deref;

use crossbeam::channel::{SendError, Sender};
#[cfg(feature = "metrics")]
use prometheus::Gauge;

#[derive(Clone, Debug)]
pub struct MassaSender<T> {
    pub(crate) sender: Sender<T>,
    pub(crate) name: String,
    /// channel size
    #[cfg(feature = "metrics")]
    pub(crate) actual_len: Gauge,
}

impl<T> MassaSender<T> {
    /// Send a message to the channel
    pub fn send(&self, msg: T) -> Result<(), SendError<T>> {
        match self.sender.send(msg) {
            Ok(()) => {
                #[cfg(feature = "metrics")]
                {
                    self.actual_len.inc();
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
