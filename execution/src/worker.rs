use crate::config::ExecutionConfig;
use crate::error::ExecutionError;
use crate::vm::VM;
use massa_hash::hash::Hash;
use models::address::AddressHashMap;
use models::hhasher::{BuildHHasher, HHashMap};
use models::{Address, Amount, Block, BlockHashMap, BlockId, OperationType, Slot};
use parking_lot::{Condvar, Mutex};
use std::mem;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tokio::sync::mpsc;
use wasmer::{Module, Store};

/// Commands sent to the `execution` component.
#[derive(Debug)]
pub enum ExecutionCommand {
    /// The clique has changed,
    /// contains the blocks of the new blockclique
    /// and a list of blocks that became final
    BlockCliqueChanged {
        blockclique: BlockHashMap<Block>,
        finalized_blocks: BlockHashMap<Block>,
    },
}

// Events produced by the execution component.
pub enum ExecutionEvent {
    /// A coin transfer
    /// from the SCE ledger to the CSS ledger.
    TransferToConsensus,
}

/// Management commands sent to the `execution` component.
pub enum ExecutionManagementCommand {}

#[derive(Debug, Clone)]
struct SCELedgerEntry {
    balance: Amount,
    bytecode: Vec<u8>,
    data: HHashMap<Hash, Vec<u8>>,
}

/// Execution queue.
pub enum ExecutionQueue {
    Running(Vec<Module>),
    Shutdown,
    Stopped,
}

pub struct ExecutionWorker {
    /// Configuration
    _cfg: ExecutionConfig,
    /// Thread count
    thread_count: u8,
    /// Receiver of commands.
    controller_command_rx: mpsc::Receiver<ExecutionCommand>,
    /// Receiver of management commands.
    controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
    /// Sender of events.
    _event_sender: mpsc::UnboundedSender<ExecutionEvent>,
    /// Time cursors
    last_final_slot: Slot,
    last_active_slot: Slot,
    /// SCE ledger
    final_ledger: AddressHashMap<SCELedgerEntry>,
    active_ledger: AddressHashMap<SCELedgerEntry>,
    /// ordered active blocks
    ordered_active_blocks: Vec<(BlockId, Block)>,
    /// pending CSS final blocks
    ordered_pending_css_final_blocks: Vec<(BlockId, Block)>,
    /// Execution queue.
    execution_queue: Arc<(Mutex<ExecutionQueue>, Condvar)>,
    /// VM thread join handle.
    vm_join_handle: JoinHandle<()>,
    /// VM Store, shared with the VM.
    store: Arc<Store>,
    /// Ledger, shared with the VM.
    /// TODO: use SCELedgerEntry and final/active concepts.
    ledger: Arc<Mutex<AddressHashMap<Module>>>,
}

impl ExecutionWorker {
    pub async fn new(
        cfg: ExecutionConfig,
        thread_count: u8,
        event_sender: mpsc::UnboundedSender<ExecutionEvent>,
        controller_command_rx: mpsc::Receiver<ExecutionCommand>,
        controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
    ) -> Result<ExecutionWorker, ExecutionError> {
        // Shared with the VM.
        let execution_queue = Arc::new((
            Mutex::new(ExecutionQueue::Running(Default::default())),
            Condvar::new(),
        ));
        let execution_queue_clone = execution_queue.clone();

        let ledger = Arc::new(Mutex::new(AddressHashMap::with_capacity_and_hasher(
            1,
            BuildHHasher::default(),
        )));
        let ledger_clone = ledger.clone();

        let store = Arc::new(Store::default());
        let store_clone = store.clone();

        let vm_join_handle = thread::spawn(move || {
            let vm = VM::new(store_clone, ledger_clone);
            loop {
                let modules = {
                    // Scoping the lock.
                    let &(ref lock, ref condvar) = &*execution_queue_clone;
                    let mut queue = lock.lock();

                    // Run until shutdown
                    loop {
                        condvar.wait(&mut queue);

                        // Running normally.
                        match *queue {
                            ExecutionQueue::Running(ref mut queue) => {
                                // Return the modules to run.
                                break mem::take(queue);
                            }
                            ExecutionQueue::Stopped => panic!("Unexpected execution queue state."),
                            ExecutionQueue::Shutdown => {
                                // Confirm shutdown
                                *queue = ExecutionQueue::Stopped;
                                condvar.notify_one();

                                // Dropping the lock.
                                return;
                            }
                        }
                    }
                };

                // Run stuff without holding the lock.
                vm.run(&modules);
            }
        });

        let worker = ExecutionWorker {
            _cfg: cfg,
            thread_count,
            controller_command_rx,
            controller_manager_rx,
            _event_sender: event_sender,
            //TODO bootstrap or init
            last_final_slot: Slot::new(0, 0),
            last_active_slot: Slot::new(0, 0),
            final_ledger: Default::default(),
            active_ledger: Default::default(),
            ordered_active_blocks: Default::default(),
            ordered_pending_css_final_blocks: Default::default(),
            execution_queue,
            vm_join_handle,
            store,
            ledger,
        };

        Ok(worker)
    }

    pub async fn run_loop(mut self) -> Result<(), ExecutionError> {
        loop {
            tokio::select! {
                // Process management commands
                cmd = self.controller_manager_rx.recv() => {
                    match cmd {
                    None => break,
                    Some(_) => {}
                }}

                // Process commands
                Some(cmd) = self.controller_command_rx.recv() => self.process_command(cmd).await?,
            }
        }

        // Signal shutdown.
        let &(ref lock, ref condvar) = &*self.execution_queue;
        let mut queue = lock.lock();
        *queue = ExecutionQueue::Shutdown;
        condvar.notify_one();

        // Wait for shutdown confirmation.
        loop {
            match *queue {
                ExecutionQueue::Stopped => break,
                _ => {}
            }
            condvar.wait(&mut queue);
        }

        // Join on the thread, once shutdown has been confirmed.
        self.vm_join_handle.join();

        // end loop
        Ok(())
    }

    fn apply_final_slot(
        &mut self,
        slot: Slot,
        opt_block: Option<(BlockId, &Block)>,
    ) -> Result<(), ExecutionError> {
        // TODO
        Ok(())
    }

    fn apply_active_slot(
        &mut self,
        slot: Slot,
        opt_block: Option<(BlockId, &Block)>,
    ) -> Result<(), ExecutionError> {
        // TODO
        Ok(())
    }

    /// Process a given command.
    ///
    /// # Argument
    /// * cmd: command to process
    async fn process_command(&mut self, cmd: ExecutionCommand) -> Result<(), ExecutionError> {
        match cmd {
            ExecutionCommand::BlockCliqueChanged {
                blockclique,
                finalized_blocks,
            } => {
                self.blockclique_changed(blockclique, finalized_blocks)?;
            }
        }
        Ok(())
    }

    /// Execute blocks that are "ready for execution".
    fn execute_blocks(&mut self, blocks: Vec<(BlockId, Block)>) -> Result<(), ExecutionError> {
        // Add new SCE final blocks to the run queue.
        let &(ref lock, ref condvar) = &*self.execution_queue;
        let mut queue = lock.lock();
        let mut ledger = self.ledger.lock();
        match *queue {
            ExecutionQueue::Running(ref mut queue) => {
                // TODO: use Damir's code to get the blocks that are "ready for execution".
                for (_, block) in blocks.into_iter() {
                    for operation in block.operations {
                        if let OperationType::ExecuteSC { data, .. } = operation.content.op {
                            let address =
                                Address::from_public_key(&operation.content.sender_public_key)
                                    .unwrap();
                            // Compile the module.
                            let module =
                                Module::new(&self.store, &data).expect("Failed to compile.");
                            // Add compiled module to the ledger.
                            ledger.insert(address, module.clone());
                            // Add module to the run queue.
                            queue.push(module);
                        }
                    }
                }
            }
            _ => panic!("Unexpected state."),
        }

        // Notify the VM.
        condvar.notify_one();

        Ok(())
    }

    fn blockclique_changed(
        &mut self,
        blockclique: BlockHashMap<Block>,
        finalized_blocks: BlockHashMap<Block>,
    ) -> Result<(), ExecutionError> {
        // gather pending finalized CSS
        let mut css_final_blocks: Vec<(BlockId, Block)> = self
            .ordered_pending_css_final_blocks
            .drain(..)
            .chain(finalized_blocks.into_iter())
            .collect();
        css_final_blocks.sort_unstable_by_key(|(_, b)| b.header.content.slot);

        // list maximum thread slots
        let mut max_thread_slot = vec![self.last_final_slot; self.thread_count as usize];
        for (_b_id, block) in css_final_blocks.iter() {
            max_thread_slot[block.header.content.slot.thread as usize] = std::cmp::max(
                max_thread_slot[block.header.content.slot.thread as usize],
                block.header.content.slot,
            );
        }

        // list final blocks
        for (b_id, block) in css_final_blocks.into_iter() {
            let block_slot = block.header.content.slot;
            if block_slot <= self.last_final_slot {
                continue;
            }
            loop {
                let next_final_slot = self.last_final_slot.get_next_slot(self.thread_count)?;
                if block_slot == next_final_slot {
                    self.apply_final_slot(next_final_slot, Some((b_id, &block)))?;
                } else if next_final_slot < max_thread_slot[next_final_slot.thread as usize] {
                    self.apply_final_slot(next_final_slot, None)?;
                } else {
                    self.ordered_pending_css_final_blocks.push((b_id, block));
                    break;
                }
                self.last_final_slot = next_final_slot;
            }
        }

        // New blocks.
        let new_blocks: Vec<(BlockId, Block)> = blockclique
            .into_iter()
            .filter(|(_b_id, b)| b.header.content.slot > self.last_final_slot)
            .collect();

        // list remaining CSS finals + new blockclique
        self.ordered_active_blocks = self
            .ordered_pending_css_final_blocks
            .iter()
            .cloned()
            .chain(new_blocks.clone().into_iter())
            .collect();

        // sort active blocks
        self.ordered_active_blocks
            .sort_unstable_by_key(|(_b_id, b)| b.header.content.slot);

        // apply active blocks and misses to the active ledger
        self.active_ledger = self.final_ledger.clone();
        self.last_active_slot = self.last_final_slot;
        // TODO: remove clone.
        for (b_id, block) in self.ordered_active_blocks.clone() {
            // process misses
            if self.last_active_slot == self.last_final_slot {
                self.last_active_slot = self.last_active_slot.get_next_slot(self.thread_count)?;
            }
            while self.last_active_slot < block.header.content.slot {
                self.apply_active_slot(self.last_active_slot, None)?;
                self.last_active_slot = self.last_active_slot.get_next_slot(self.thread_count)?;
            }
            self.apply_active_slot(self.last_active_slot, Some((b_id, &block)))?;
        }

        // Execute blocks.
        self.execute_blocks(new_blocks)?;

        Ok(())
    }
}
