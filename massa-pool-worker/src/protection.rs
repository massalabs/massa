use std::{
    cmp::Ordering,
    collections::{btree_set::Iter, BTreeSet, VecDeque},
    iter::Rev,
    sync::{mpsc, Arc},
    thread::JoinHandle,
    time::Duration,
};

use massa_execution_exports::ExecutionController;
use massa_models::{
    address::Address,
    amount::Amount,
    operation::OperationId,
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
};
use massa_pool_exports::ProtectionController;
use massa_time::MassaTime;
use parking_lot::RwLock;
use std::collections::hash_map::Entry;

use crate::{operation_pool::OperationPool, types::PoolOperationCursor};

/// Protection controller, handle the protection thread and
/// allow to stop the pool protection.
pub struct ProtectionControllerImpl {
    pub(crate) stop_sender: mpsc::Sender<()>,
    pub(crate) thread: Option<JoinHandle<()>>,
}

impl ProtectionController for ProtectionControllerImpl {
    fn stop(&mut self) {
        self.stop_sender
            .send(())
            .expect("unable to stop the protection thread");
        self.thread
            .take()
            .expect("expected a thread into the pool protection controller")
            .join()
            .expect("unable to join the protection thread");
    }
}

pub(crate) struct PoolAddrInfoWS {
    /// Order of the verification priority from highest to lowest (head to rear)
    queue: VecDeque<Address>,
    /// Store information by address
    infos: PreHashMap<Address, PoolAddrInfo>,
    /// Latest takes
    taken: PreHashSet<Address>,
}

impl PoolAddrInfoWS {
    pub fn new(batch_size: usize) -> Self {
        Self {
            queue: Default::default(),
            infos: Default::default(),
            taken: PreHashSet::with_capacity(batch_size),
        }
    }

    /// Insert or update information in the pool protection workspace
    pub fn insert_or_update(&mut self, address: Address, max_spending: Amount, op_id: OperationId) {
        match self.infos.entry(address) {
            Entry::Occupied(e) => {
                let i = e.get_mut();
                i.ops.insert(op_id);
                i.max_spending = max_spending;
            }
            Entry::Vacant(mut e) => {
                e.insert(PoolAddrInfo {
                    max_spending: Amount::zero(),
                    ops: PreHashSet::from_iter([op_id]),
                    last_robot_scan: None,
                });
                // First operation in pool imply a low priority for verification
                self.queue.push_back(address)
            }
        };
    }

    pub fn take(&mut self, batch_size: usize) -> PreHashSet<Address> {
        //let mut ret = PreHashSet::with_capacity(batch_size);
        todo!()
    }
}

/// Address pool info container
pub(crate) struct PoolAddrInfo {
    /// is the cumulated max balance spending (op.get_max_sequential_spending()) of all ops emitted by this address
    pub(crate) max_spending: Amount,
    /// Operations created by that address in the pool
    /// Note: may be a useless duplication of the "pool storage" -> "index by addresses". An investigation is wellcome.
    pub(crate) ops: PreHashSet<OperationId>,
    /// is None if there was no scan, and otherwise provides the pair (last_balance_retrieval_time, retrieved_balance)
    last_robot_scan: Option<(MassaTime, Amount)>,
}

pub(crate) struct ProtectionConfig {
    pub(crate) batch_size: usize,
    pub(crate) roll_price: Amount,
    pub(crate) timeout: Duration,
}

/// take a batch of addresses in index_by_address among the ones for which
/// last_robot_scan is the lowest (None is lower than Some(_))
fn get_addresses_to_check(
    batch_size: usize,
    index_by_address: &Arc<RwLock<PreHashMap<Address, PoolAddrInfo>>>,
) -> PreHashSet<Address> {
    let index_by_address = index_by_address.read();
    if index_by_address.is_empty() {
        return PreHashSet::default();
    }
    // Implementation with 1 structure in O(n log n)
    let mut oldest_checked_addresses: Vec<(&Address, &PoolAddrInfo)> =
        index_by_address.iter().collect();
    oldest_checked_addresses.sort_by(|a, b| match (a.1.last_robot_scan, b.1.last_robot_scan) {
        (Some(a), Some(b)) => a.0.cmp(&b.0),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (_, _) => Ordering::Equal,
    });

    PreHashSet::from_iter(
        oldest_checked_addresses
            .into_iter()
            .take(batch_size)
            .map(|(addr, _)| *addr),
    )
}

/// Reset/Init a batch of addresses to check with a maximal size of `cfg.batch_size`
///
/// # Return
/// Set of addresses to check in the execution
fn reset_batch_info(
    execution_controller: &dyn ExecutionController,
    index_by_address: &Arc<RwLock<PreHashMap<Address, PoolAddrInfo>>>,
    batch_size: usize, // use cfg
) -> PreHashSet<Address> {
    let addresses = get_addresses_to_check(batch_size, index_by_address);

    let now = MassaTime::now(0).unwrap(); // No compensaion needed
    let index_by_address = &mut index_by_address.write();
    for (ex_addr_info, addr) in execution_controller
        .get_addresses_infos(&addresses.iter().cloned().collect::<Vec<_>>())
        .iter()
        .zip(addresses.iter())
    {
        if let Some(info) = index_by_address.get_mut(addr) {
            // update address_info.last_robot_scan with
            // the current date and retrieved balance.
            info.last_robot_scan = Some((now, ex_addr_info.final_balance));
        }
    }
    addresses
}

/// Iterator to always take the lowest operation for all threads
struct SortedOperationIterator<'a> {
    nexts: Vec<Option<&'a PoolOperationCursor>>,
    iters: Vec<Rev<Iter<'a, PoolOperationCursor>>>,
}

impl<'a> SortedOperationIterator<'a> {
    /// Create an iterato over all operations. Look from the lowest RoI
    /// to the best one.
    fn new(ops_sorted_by_threads: &'a [BTreeSet<PoolOperationCursor>]) -> Self {
        let mut iters = vec![];
        let mut nexts = vec![];
        for p in ops_sorted_by_threads.iter() {
            let mut it = p.iter().rev();
            nexts.push(it.next());
            iters.push(it);
        }

        Self { nexts, iters }
    }
}

impl<'a> Iterator for SortedOperationIterator<'a> {
    type Item = &'a PoolOperationCursor;

    fn next(&mut self) -> Option<Self::Item> {
        let mut min = self.nexts[0];
        let mut min_index = 0;
        for i in 0..self.nexts.len() {
            if self.nexts[i] < min || min.is_none() {
                min = self.nexts[i];
                min_index = i;
            }
        }
        if min.is_none() {
            return min;
        }

        self.nexts[min_index] = self.iters[min_index].next();
        min
    }
}

fn process_protection(
    operation_pool: &Arc<RwLock<OperationPool>>,
    index_by_address: &Arc<RwLock<PreHashMap<Address, PoolAddrInfo>>>,
    addresses: PreHashSet<Address>,
    cfg: &ProtectionConfig,
) {
    let pool_writer = &mut *operation_pool.write();
    let mut to_remove = PreHashSet::default();
    let mut overriding_addresses = PreHashSet::default();
    let index_by_address_writer = &mut index_by_address.write();

    {
        let ops_reader = pool_writer.storage.read_operations();

        // Initialize the iterator over all operations. Take only those with
        // an address to check as creator.
        let ops = SortedOperationIterator::new(&pool_writer.sorted_ops_per_thread)
            .filter_map(|cursor| ops_reader.get(&cursor.get_id()))
            .filter(|op| addresses.contains(&op.creator_address));

        // enhancement proposal:
        //       In case of a duration longer than a given timeout value could
        //       abort the check of the rest of the list.
        for op in ops {
            let op_max_spending = op.get_max_spending(cfg.roll_price);

            let mut addr_info = match index_by_address_writer.entry(op.creator_address) {
                Entry::Occupied(e) => e,
                _ => return,
            };

            if overriding_addresses.contains(&op.creator_address) {
                // Quick remove. The entry `addr_info` can't be empty here
                to_remove.insert(op.id);
                addr_info.get_mut().ops.remove(&op.id);
            }

            addr_info.get_mut().max_spending =
                addr_info.get().max_spending.saturating_sub(op_max_spending);

            if addr_info.get().max_spending > addr_info.get().last_robot_scan.unwrap().1 {
                // while address_info.max_spending is strictly higher than the balance:
                //     drop the least prioritary op among address_info.ops from the pool
                //     remove it from address_info.ops and substract its max spending from address_info.max_spending
                // if there are no more elements in address_info.ops, delete the complete address_info entry
                addr_info.get_mut().ops.remove(&op.id);
                if addr_info.get_mut().ops.is_empty() {
                    addr_info.remove();
                }
                to_remove.insert(op.id);
                // Add a shurtcut to avoid useless operations
                overriding_addresses.insert(op.creator_address);
            }
        }
    }

    // Clean the pool and its storage
    for id in to_remove.iter() {
        let op_info = match pool_writer.operations.remove(id) {
            Some(info) => info,
            _ => continue,
        };
        if !pool_writer.sorted_ops_per_thread[op_info.thread as usize].remove(&op_info.cursor) {
            panic!("expected op presence in sorted list")
        }
    }
    pool_writer.storage.drop_operation_refs(&to_remove);
}

/// Start the Protection thread.
///
/// Prune monotonically overflow of operations when the total fees claimed by
/// an address is strickly higher than it sequencial balance.
pub(crate) fn start_protection_thread(
    operation_pool: Arc<RwLock<OperationPool>>,
    index_by_address: Arc<RwLock<PreHashMap<Address, PoolAddrInfo>>>,
    execution_controller: Box<dyn ExecutionController>,
    channel: mpsc::Receiver<()>, // stop handler
    cfg: ProtectionConfig,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            if channel.recv_timeout(cfg.timeout).is_ok() {
                break; // stop msg received
            }

            let addresses =
                reset_batch_info(&*execution_controller, &index_by_address, cfg.batch_size);

            if addresses.is_empty() {
                // No addresses to check, pass
                continue;
            }

            process_protection(&operation_pool, &index_by_address, addresses, &cfg);
        }
    })
}
