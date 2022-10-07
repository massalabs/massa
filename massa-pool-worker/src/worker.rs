use std::sync::mpsc::Receiver;

use massa_pool_exports::{PoolController, PoolError, PoolManager};

use crate::controller_impl::Command;

pub(crate) struct SelectorThread(Receiver<Command>);

/// TODO
pub fn start_pool_worker() -> Result<(Box<dyn PoolManager>, Box<dyn PoolController>), PoolError> {}
