// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module represents an event store allowing to store, search and retrieve
//! a config-limited number of execution-generated events

use massa_models::output_event::{SCOutputEvent, SCOutputEventId};
use massa_models::prehash::{Map, PreHashed, Set};
/// Define types used while executing block bytecodes
use massa_models::{Address, OperationId, Slot};
use std::cmp;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use tracing::warn;

#[inline]
/// Remove a given event_id from a `Set<SCOutputEventId>`
/// The Set is stored into a map `ctnr` at a `key` address. If
/// the Set resulted from the operation is empty, remove the entry
/// from the `ctnr`
///
/// Used in `prune()`
fn remove_from_map<T: Eq + std::hash::Hash + PreHashed>(
    ctnr: &mut Map<T, Set<SCOutputEventId>>,
    key: &T,
    evt_id: &SCOutputEventId,
) {
    match ctnr.get_mut(key) {
        Some(ele) => {
            ele.remove(evt_id);
            if ele.is_empty() {
                ctnr.remove(key);
            }
        }
        _ => {
            ctnr.remove(key);
        }
    }
}

#[inline]
/// Remove a given event_id from a `Set<SCOutputEventId>`
/// The Set is stored into a Hashmap `ctnr` at a `key` address. If
/// the Set resulted from the operation is empty, remove the entry
/// from the `ctnr`
///
/// Used in `prune()`
fn remove_from_hashmap<T: Eq + std::hash::Hash>(
    ctnr: &mut HashMap<T, Set<SCOutputEventId>>,
    key: &T,
    evt_id: &SCOutputEventId,
) {
    match ctnr.get_mut(key) {
        Some(ele) => {
            ele.remove(evt_id);
            if ele.is_empty() {
                ctnr.remove(key);
            }
        }
        _ => {
            ctnr.remove(key);
        }
    }
}

/// Keep all events you need with some useful indexes
#[derive(Default, Debug, Clone)]
pub struct EventStore {
    /// maps ids to events
    id_to_event: Map<SCOutputEventId, SCOutputEvent>,

    /// maps slot to a set of event ids
    slot_to_id: HashMap<Slot, Set<SCOutputEventId>>,

    /// maps initial caller to a set of event ids
    caller_to_id: Map<Address, Set<SCOutputEventId>>,

    /// maps direct event producer to a set of event ids
    smart_contract_to_id: Map<Address, Set<SCOutputEventId>>,

    /// maps operation id to a set of event ids
    operation_id_to_event_id: Map<OperationId, Set<SCOutputEventId>>,
}

impl EventStore {
    /// add event to the store and all its indexes
    pub fn insert(&mut self, id: SCOutputEventId, event: SCOutputEvent) {
        if let Entry::Vacant(entry) = self.id_to_event.entry(id) {
            self.slot_to_id
                .entry(event.context.slot)
                .or_insert_with(Set::<SCOutputEventId>::default)
                .insert(id);
            if let Some(&caller) = event.context.call_stack.front() {
                self.caller_to_id
                    .entry(caller)
                    .or_insert_with(Set::<SCOutputEventId>::default)
                    .insert(id);
            }
            if let Some(&sc) = event.context.call_stack.back() {
                self.smart_contract_to_id
                    .entry(sc)
                    .or_insert_with(Set::<SCOutputEventId>::default)
                    .insert(id);
            }
            if let Some(op) = event.context.origin_operation_id {
                self.operation_id_to_event_id
                    .entry(op)
                    .or_insert_with(Set::<SCOutputEventId>::default)
                    .insert(id);
            }
            entry.insert(event);
        } else {
            // just warn or return error ?
            warn!("execution event already exist {:?}", id)
        }
    }

    /// get just the map if ids to events
    pub fn export(&self) -> Map<SCOutputEventId, SCOutputEvent> {
        self.id_to_event.clone()
    }

    /// Clears the map, removing all key-value pairs. Keeps the allocated memory for reuse.
    pub fn clear(&mut self) {
        self.id_to_event.clear();
        self.slot_to_id.clear();
        self.caller_to_id.clear();
        self.smart_contract_to_id.clear();
        self.operation_id_to_event_id.clear();
    }

    /// Prune the exess of events from the event store,
    /// While there is a slot found, pop slots and get the `event_ids`
    /// inside, remove the event from divers containers.
    ///
    /// Return directly if the event_size <= max_final_events
    pub fn prune(&mut self, max_final_events: usize) {
        let mut events_size = self.id_to_event.len();
        if events_size <= max_final_events {
            return;
        }
        let mut slots = self.slot_to_id.keys().copied().collect::<Vec<_>>();
        slots.sort_unstable_by_key(|s| cmp::Reverse(*s));
        loop {
            let slot = match slots.pop() {
                Some(slot) => slot,
                _ => return,
            };
            let event_ids = match self.slot_to_id.get(&slot) {
                Some(event_ids) => event_ids.clone(),
                _ => continue,
            };
            for event_id in event_ids.iter() {
                let event = match self.id_to_event.remove(event_id) {
                    Some(event) => event,
                    _ => continue, /* This shouldn't happen */
                };
                remove_from_hashmap(&mut self.slot_to_id, &event.context.slot, event_id);
                if let Some(caller) = event.context.call_stack.front() {
                    remove_from_map(&mut self.caller_to_id, caller, event_id);
                }
                if let Some(sc) = event.context.call_stack.back() {
                    remove_from_map(&mut self.smart_contract_to_id, sc, event_id);
                }
                if let Some(op) = event.context.origin_operation_id {
                    remove_from_map(&mut self.operation_id_to_event_id, &op, event_id);
                }
                events_size -= 1;
                if events_size <= max_final_events {
                    return;
                }
            }
        }
    }

    /// Extend an event store with another one
    pub fn extend(&mut self, other: EventStore) {
        self.id_to_event.extend(other.id_to_event);

        other
            .slot_to_id
            .iter()
            .for_each(|(slot, ids)| match self.slot_to_id.get_mut(slot) {
                Some(set) => set.extend(ids),
                None => {
                    self.slot_to_id.insert(*slot, ids.clone());
                }
            });

        other.caller_to_id.iter().for_each(|(caller, ids)| {
            match self.caller_to_id.get_mut(caller) {
                Some(set) => set.extend(ids),
                None => {
                    self.caller_to_id.insert(*caller, ids.clone());
                }
            }
        });

        other.smart_contract_to_id.iter().for_each(|(sc, ids)| {
            match self.smart_contract_to_id.get_mut(sc) {
                Some(set) => set.extend(ids),
                None => {
                    self.smart_contract_to_id.insert(*sc, ids.clone());
                }
            }
        });

        other.operation_id_to_event_id.iter().for_each(|(op, ids)| {
            match self.operation_id_to_event_id.get_mut(op) {
                Some(set) => set.extend(ids),
                None => {
                    self.operation_id_to_event_id.insert(*op, ids.clone());
                }
            }
        })
    }

    /// get vec of event for given slot range (start included, end excluded)
    /// Get events optionnally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    pub fn get_filtered_sc_output_event(
        &self,
        start: Slot,
        end: Slot,
        emitter_address: Option<Address>,
        original_caller_address: Option<Address>,
        original_operation_id: Option<OperationId>,
    ) -> Vec<SCOutputEvent> {
        let empty = Set::<SCOutputEventId>::default();
        self.slot_to_id
            .iter()
            // filter on slots
            .filter_map(|(slot, ids)| {
                if slot >= &start && slot < &end {
                    Some(ids)
                } else {
                    None
                }
            })
            .flatten()
            // filter on original caller
            .chain(if let Some(addr) = original_caller_address {
                match self.caller_to_id.get(&addr) {
                    Some(it) => it.iter(),
                    None => empty.iter(),
                }
            } else {
                empty.iter()
            })
            // filter on emitter
            .chain(if let Some(addr) = emitter_address {
                match self.smart_contract_to_id.get(&addr) {
                    Some(it) => it.iter(),
                    None => empty.iter(),
                }
            } else {
                empty.iter()
            })
            // filter on operation id
            .chain(if let Some(op) = original_operation_id {
                match self.operation_id_to_event_id.get(&op) {
                    Some(it) => it.iter(),
                    None => empty.iter(),
                }
            } else {
                empty.iter()
            })
            .filter_map(|id| self.id_to_event.get(id))
            .cloned()
            .collect()
    }
}
