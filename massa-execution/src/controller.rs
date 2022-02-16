use crate::{config::VMConfig, types::ReadOnlyExecutionRequest, vm_thread::VMThread};
use massa_ledger::FinalLedger;
use massa_models::{Block, BlockId, Slot};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use tracing::info;

/// structure used to communicate with the VM thread
#[derive(Default)]
pub struct VMInputData {
    /// set stop to true to stop the thread
    pub stop: bool,
    /// signal whether the blockclique changed
    pub blockclique_changed: bool,
    /// list of newly finalized blocks
    pub finalized_blocks: HashMap<Slot, (BlockId, Block)>,
    /// blockclique
    pub blockclique: HashMap<Slot, (BlockId, Block)>,
    /// readonly execution requests
    pub readonly_requests: VecDeque<ReadOnlyExecutionRequest>,
}

/// VM controller
pub struct VMController {
    /// condition variable to wake up the VM loop
    pub loop_cv: Condvar,
    /// input data to process in the VM loop
    pub input_data: Mutex<VMInputData>,
}

impl VMController {
    /// reads the list of newly finalized blocks and the new blockclique, if there was a change
    /// if found, remove from input queue
    pub fn consume_input(&mut self) -> VMInputData {
        std::mem::take(&mut self.input_data.lock().expect("VM input data lock failed"))
    }
}

/// VM manager
pub struct VMManager {
    /// shared reference to the VM controller
    controller: Arc<VMController>,
    /// handle used to join the VM thread
    thread_handle: std::thread::JoinHandle<()>,
}

impl VMManager {
    /// stops the VM
    pub fn stop(self) {
        info!("stopping VM controller...");
        // notify the VM thread to stop
        {
            let mut input_wlock = self
                .controller
                .input_data
                .lock()
                .expect("could not w-lock VM input data");
            input_wlock.stop = true;
            self.controller.loop_cv.notify_one();
        }
        // join the VM thread
        self.thread_handle
            .join()
            .expect("VM controller thread panicked");
        info!("VM controller stopped");
    }

    /// get a shared reference to the VM controller
    pub fn get_controller(&self) -> Arc<VMController> {
        self.controller.clone()
    }
}

/// launches the VM and returns a VMManager
///
/// # parameters
/// * config: VM configuration
/// * bootstrap:
pub fn start_vm(config: VMConfig, final_ledger: Arc<RwLock<FinalLedger>>) -> VMManager {
    let controller = Arc::new(VMController {
        loop_cv: Condvar::new(),
        input_data: Mutex::new(VMInputData {
            blockclique_changed: true,
            ..Default::default()
        }),
    });

    let ctl = controller.clone();
    let thread_handle = std::thread::spawn(move || {
        VMThread::new(config, ctl, final_ledger).main_loop();
    });

    VMManager {
        controller,
        thread_handle,
    }
}
