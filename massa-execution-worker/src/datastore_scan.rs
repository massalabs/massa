//! A file describing an optimized datastore keys traversal algorithm.
//! It is shared between execution.rs and speculative_ledger.rs.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Bound,
    sync::Arc,
};

use massa_final_state::FinalStateController;
use massa_ledger_exports::LedgerChanges;
use massa_models::{
    address::Address,
    datastore::{get_prefix_bounds, range_intersection, Datastore},
    types::{SetOrDelete, SetUpdateOrDelete},
};
use parking_lot::RwLock;

use crate::active_history::ActiveHistory;

/// Builds the bounded result for a `Set` reset by streaming the absolute
/// datastore in key order and overlaying the newer-layer updates, stopping as
/// soon as `count` surviving keys are collected. A naive truncation of the
/// absolute range would be wrong: a newer update may delete one of the first
/// `count` keys, which must then be replaced by the next key.
#[allow(clippy::type_complexity)]
fn merge_set_case_keys(
    absolute_set_datastore: Option<&Datastore>,
    key_updates: &BTreeMap<Vec<u8>, bool>,
    key_range: Option<&(Bound<Vec<u8>>, Bound<Vec<u8>>)>,
    count: Option<u32>,
) -> BTreeSet<Vec<u8>> {
    let mut out = BTreeSet::new();
    match absolute_set_datastore {
        Some(abs) => {
            if let Some(k_range) = key_range {
                let mut abs_it = abs.range(k_range.clone()).peekable();
                let mut upd_it = key_updates.iter().peekable();
                loop {
                    if let Some(cnt) = count {
                        if out.len() >= cnt as usize {
                            break;
                        }
                    }
                    match (abs_it.peek(), upd_it.peek()) {
                        (Some((abs_key, _)), Some((upd_key, is_set))) => {
                            match abs_key.cmp(upd_key) {
                                Ordering::Less => {
                                    out.insert(abs_key.to_vec());
                                    abs_it.next();
                                }
                                Ordering::Equal => {
                                    if **is_set {
                                        out.insert(abs_key.to_vec());
                                    }
                                    abs_it.next();
                                    upd_it.next();
                                }
                                Ordering::Greater => {
                                    if **is_set {
                                        out.insert(upd_key.to_vec());
                                    }
                                    upd_it.next();
                                }
                            }
                        }
                        (Some((abs_key, _)), None) => {
                            out.insert(abs_key.to_vec());
                            abs_it.next();
                        }
                        (None, Some((upd_key, is_set))) => {
                            if **is_set {
                                out.insert(upd_key.to_vec());
                            }
                            upd_it.next();
                        }
                        (None, None) => break,
                    }
                }
            }
        }
        None => {
            // reset after a speculative delete: updates are the only source
            for (k, is_set) in key_updates.iter() {
                if *is_set {
                    out.insert(k.clone());
                    if let Some(cnt) = count {
                        if out.len() >= cnt as usize {
                            break;
                        }
                    }
                }
            }
        }
    }
    out
}

/// Gets a copy of a datastore keys for a given address
///
/// # Arguments
/// * `addr`: address to query
/// * `prefix`: prefix to filter keys
/// * `start_key`: start key of the range
/// * `end_key`: end key of the range
/// * `count`: maximum number of keys to return
///
/// # Returns
/// A tuple of two `Option<BTreeSet<Vec<u8>>>`:
/// `None` means that the address does not exist.
/// The first element is the final state keys, the second element is the speculative keys.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn scan_datastore(
    addr: &Address,
    prefix: &[u8],
    start_key: Bound<Vec<u8>>,
    end_key: Bound<Vec<u8>>,
    count: Option<u32>,
    final_state: Arc<RwLock<dyn FinalStateController>>,
    active_history: Arc<RwLock<ActiveHistory>>,
    added_changes: Option<&LedgerChanges>,
) -> (Option<BTreeSet<Vec<u8>>>, Option<BTreeSet<Vec<u8>>>) {
    // get final keys
    let final_keys = final_state.read().get_ledger().get_datastore_keys(
        addr,
        prefix,
        start_key.clone(),
        end_key.clone(),
        count,
    );

    // the iteration range is the intersection of the prefix range and the selection range
    let key_range = range_intersection(
        get_prefix_bounds(prefix),
        (start_key.clone(), end_key.clone()),
    );

    enum SpeculativeResetType {
        None,
        Set,
        Delete,
    }

    // process speculative history
    let mut speculative_reset = SpeculativeResetType::None;
    let mut key_updates = BTreeMap::new();
    // For a `Set` reset, the absolute datastore is streamed and merged with the newer
    // updates below instead of being copied here, so a bounded query cannot
    // materialise a whole large datastore.
    let mut absolute_set_datastore: Option<&Datastore> = None;
    // Bounded result for a `Set` reset, computed below.
    let mut set_case_keys = None;
    {
        let mut update_indices = VecDeque::new();
        let history_lock = active_history.read();

        let it = history_lock
            .0
            .iter()
            .map(|v| &v.state_changes.ledger_changes)
            .chain(added_changes.iter().copied());
        let mut index = history_lock.0.len() + if added_changes.is_some() { 1 } else { 0 };
        for output in it.rev() {
            index -= 1;
            match output.get(addr) {
                // address absent from the changes
                None => (),

                // address ledger entry being reset to an absolute new list of keys
                Some(SetUpdateOrDelete::Set(v)) => {
                    absolute_set_datastore = Some(&v.datastore);
                    speculative_reset = SpeculativeResetType::Set;
                    break;
                }

                // address ledger entry being updated within the key range of interest
                Some(SetUpdateOrDelete::Update(updates)) => {
                    if let Some(k_range) = key_range.as_ref() {
                        if updates.datastore.range(k_range.clone()).next().is_some() {
                            update_indices.push_front(index);
                        }
                    }
                }

                // address ledger entry being deleted
                Some(SetUpdateOrDelete::Delete) => {
                    speculative_reset = SpeculativeResetType::Delete;
                    break;
                }
            }
        }
        if matches!(speculative_reset, SpeculativeResetType::Delete) && !update_indices.is_empty() {
            // if there are updates after an address deletion, consider it a Set
            speculative_reset = SpeculativeResetType::Set;
        }

        // aggregate key updates
        for idx in update_indices {
            let changes = if idx < history_lock.0.len() {
                &history_lock.0[idx].state_changes.ledger_changes
            } else if let Some(added_changes) = added_changes.as_ref() {
                *added_changes
            } else {
                panic!("unexpected index out of bounds")
            };

            if let SetUpdateOrDelete::Update(updates) = changes
                .get(addr)
                .expect("address unexpectedly absent from the changes")
            {
                if let Some(k_range) = key_range.as_ref() {
                    for (k, update) in updates.datastore.range(k_range.clone()) {
                        match update {
                            SetOrDelete::Set(_) => {
                                key_updates.insert(k.clone(), true);
                            }
                            SetOrDelete::Delete => {
                                key_updates.insert(k.clone(), false);
                            }
                        }
                    }
                }
            } else {
                panic!("unexpected state change");
            }
        }

        if matches!(speculative_reset, SpeculativeResetType::Set) {
            set_case_keys = Some(merge_set_case_keys(
                absolute_set_datastore,
                &key_updates,
                key_range.as_ref(),
                count,
            ));
        }
    }

    // process reset-related edge cases
    match speculative_reset {
        SpeculativeResetType::Delete => {
            // the address was deleted in the speculative history without further updates
            return (final_keys, None);
        }
        SpeculativeResetType::Set => {
            // the address was reset in the speculative history: the bounded result
            // was computed while the history lock was held
            return (final_keys, set_case_keys);
        }
        SpeculativeResetType::None => {
            // there was no reset
            if key_updates.is_empty() {
                // there were no updates: return the same as final
                return (final_keys.clone(), final_keys);
            } else if final_keys.is_none() {
                // handle the case where there were updates but the final address was absent
                let filter_it =
                    key_updates
                        .into_iter()
                        .filter_map(|(k, is_set)| if is_set { Some(k) } else { None });
                if let Some(cnt) = count {
                    return (None, Some(filter_it.take(cnt as usize).collect()));
                } else {
                    return (None, Some(filter_it.collect()));
                }
            }
        }
    }

    // If we reach this point, it means that all of the following is true:
    //   * the final key list is present
    //   * there was no reset/delete in the speculative history
    //   * there were updates in the speculative history
    // This means that we need to merge the final and speculative key lists,
    // querying more final keys if necessary to reach the desired count.

    let mut final_keys_queue: VecDeque<_> = final_keys
        .as_ref()
        .expect("expected final keys to be non-None")
        .iter()
        .cloned()
        .collect();
    let mut speculative_keys: BTreeSet<_> = Default::default();
    let mut last_final_batch_key = final_keys_queue.back().cloned();
    let mut key_updates_it = key_updates.into_iter().peekable();
    loop {
        if let Some(cnt) = count {
            if speculative_keys.len() >= cnt as usize {
                return (final_keys, Some(speculative_keys));
            }
        }
        match (final_keys_queue.front(), key_updates_it.peek()) {
            (Some(_f), None) => {
                // final only
                let k = final_keys_queue
                    .pop_front()
                    .expect("expected final list to be non-empty");
                speculative_keys.insert(k);
            }
            (Some(f), Some((u, _is_set))) => {
                // key present both in the final state and as a speculative update
                match f.cmp(u) {
                    Ordering::Less => {
                        // take into account final only
                        let k = final_keys_queue
                            .pop_front()
                            .expect("expected final key queue to be non-empty");
                        speculative_keys.insert(k);
                    }
                    Ordering::Equal => {
                        // take into account the change but pop both
                        let (k, is_set) = key_updates_it
                            .next()
                            .expect("expected key update queue to be non-empty");
                        final_keys_queue.pop_front();
                        if is_set {
                            speculative_keys.insert(k);
                        }
                    }
                    Ordering::Greater => {
                        // take into account the update only
                        let (k, is_set) = key_updates_it
                            .next()
                            .expect("expected key update queue to be non-empty");
                        if is_set {
                            speculative_keys.insert(k);
                        }
                    }
                }
            }
            (None, Some((_u, _is_set))) => {
                // no final but there is a change
                let (k, is_set) = key_updates_it
                    .next()
                    .expect("expected key update queue to be non-empty");
                if is_set {
                    speculative_keys.insert(k);
                }
            }
            (None, None) => {
                // nothing is left
                return (final_keys, Some(speculative_keys));
            }
        }

        if final_keys_queue.is_empty() {
            if let Some(last_k) = last_final_batch_key.take() {
                // the last final item was consumed: replenish the queue by querying
                // only what is still missing to reach `count`, not a full batch
                let remaining = count.map(|cnt| cnt.saturating_sub(speculative_keys.len() as u32));
                final_keys_queue = final_state
                    .read()
                    .get_ledger()
                    .get_datastore_keys(
                        addr,
                        prefix,
                        std::ops::Bound::Excluded(last_k),
                        end_key.clone(),
                        remaining,
                    )
                    .expect("address expected to exist in final state")
                    .iter()
                    .cloned()
                    .collect();
                last_final_batch_key = final_keys_queue.back().cloned();
            }
        }
    }
}
