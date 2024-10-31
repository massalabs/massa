use std::cmp::max;
// std
use nom::AsBytes;
use std::path::Path;
// third-party
use crate::ser_deser::{
    SCOutputEventDeserializer, SCOutputEventDeserializerArgs, SCOutputEventSerializer,
};
use massa_models::execution::EventFilter;
use massa_models::output_event::SCOutputEvent;
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use rocksdb::{DBIterator, IteratorMode, WriteBatch, DB};
use tracing::debug;

const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const EVENT_DESER_ERROR: &str = "critical: event deserialization failed";

/// Module key formatting macro
#[macro_export]
macro_rules! event_key {
    ($event_slot:expr) => {
        [&$event_slot.to_bytes()[..], &[MODULE_IDENT]].concat()
    };
}

pub(crate) struct EventCache {
    /// RocksDB database
    db: DB,
    /// How many entries are in the db. Count is initialized at creation time by iterating
    /// over all the entries in the db then it is maintained in memory
    entry_count: usize,
    /// Maximum number of entries we want to keep in the db.
    /// When this maximum is reached `snip_amount` entries are removed
    max_entry_count: usize,
    /// How many entries are removed when `entry_count` reaches `max_entry_count`
    snip_amount: usize,
    /// Event serializer
    event_ser: SCOutputEventSerializer,
    /// Event deserializer
    event_deser: SCOutputEventDeserializer,
}

impl EventCache {
    /// Create a new EventCache
    pub fn new(path: &Path, max_entry_count: usize, snip_amount: usize, thread_count: u8) -> Self {
        let db = DB::open_default(path).expect(OPEN_ERROR);
        let entry_count = db.iterator(IteratorMode::Start).count();

        Self {
            db,
            entry_count,
            max_entry_count,
            snip_amount,
            event_ser: SCOutputEventSerializer::new(),
            event_deser: SCOutputEventDeserializer::new(SCOutputEventDeserializerArgs {
                thread_count,
            }),
        }
    }

    #[allow(dead_code)]
    /// Insert a new event in the cache
    pub fn insert(&mut self, event: SCOutputEvent) {
        if self.entry_count >= self.max_entry_count {
            self.snip(None);
        }

        let event_key = {
            let mut event_key = event.context.slot.to_bytes_key().to_vec();
            event_key.extend(event.context.index_in_slot.to_be_bytes());
            event_key
        };
        let mut event_buffer = Vec::new();
        self.event_ser.serialize(&event, &mut event_buffer).unwrap();

        let mut batch = WriteBatch::default();
        batch.put(event_key, event_buffer);
        self.db.write(batch).expect(CRUD_ERROR);
        self.entry_count = self.entry_count.saturating_add(1);

        debug!("(Event insert) entry_count is: {}", self.entry_count);
    }

    /// Insert new events in the cache
    ///
    /// For performance reason, pass events_len to avoid cloning the iterator
    pub fn insert_multi_it(
        &mut self,
        events: impl Iterator<Item = SCOutputEvent> + Clone,
        events_len: Option<usize>,
    ) {
        let events_len = events_len.unwrap_or_else(|| events.clone().count());

        if self.entry_count + events_len >= self.max_entry_count {
            let snip_amount = max(self.snip_amount, events_len);
            self.snip(Some(snip_amount));
        }

        let mut batch = WriteBatch::default();
        for event in events {
            let event_key = {
                let mut event_key = event.context.slot.to_bytes_key().to_vec();
                event_key.extend(event.context.index_in_slot.to_be_bytes());
                event_key
            };
            let mut event_buffer = Vec::new();
            self.event_ser.serialize(&event, &mut event_buffer).unwrap();

            batch.put(event_key, event_buffer);
        }
        self.db.write(batch).expect(CRUD_ERROR);
        self.entry_count = self.entry_count.saturating_add(events_len);

        debug!("(Events insert) entry_count is: {}", self.entry_count);
    }

    fn db_iter(&self, mode: Option<IteratorMode>) -> DBIterator {
        self.db.iterator(mode.unwrap_or(IteratorMode::Start))
    }

    pub(crate) fn get_filtered_sc_output_events<'b, 'a: 'b>(
        &'a self,
        filter: &'b EventFilter,
    ) -> impl Iterator<Item = SCOutputEvent> + 'b {
        self.db_iter(Some(IteratorMode::Start)).filter_map(|kvb| {
            let kvb = kvb.unwrap();
            let (_rem, event) = self
                .event_deser
                .deserialize::<DeserializeError>(kvb.1.as_bytes())
                .expect(EVENT_DESER_ERROR);

            if let Some(start) = filter.start {
                if event.context.slot < start {
                    return None;
                }
            }
            if let Some(end) = filter.end {
                if event.context.slot >= end {
                    return None;
                }
            }
            if let Some(is_final) = filter.is_final {
                if event.context.is_final != is_final {
                    return None;
                }
            }
            if let Some(is_error) = filter.is_error {
                if event.context.is_error != is_error {
                    return None;
                }
            }
            match (
                filter.original_caller_address,
                event.context.call_stack.front(),
            ) {
                (Some(addr1), Some(addr2)) if addr1 != *addr2 => return None,
                (Some(_), None) => return None,
                _ => (),
            }
            match (filter.emitter_address, event.context.call_stack.back()) {
                (Some(addr1), Some(addr2)) if addr1 != *addr2 => return None,
                (Some(_), None) => return None,
                _ => (),
            }
            match (
                filter.original_operation_id,
                event.context.origin_operation_id,
            ) {
                (Some(addr1), Some(addr2)) if addr1 != addr2 => return None,
                (Some(_), None) => return None,
                _ => (),
            }
            Some(event)
        })
    }

    /// Try to remove some entries from the db
    fn snip(&mut self, snip_amount: Option<usize>) {
        let mut iter = self.db.iterator(IteratorMode::Start);
        let mut batch = WriteBatch::default();
        let mut snipped_count: usize = 0;
        let snip_amount = snip_amount.unwrap_or(self.snip_amount);

        while snipped_count < snip_amount {
            let key_value = iter.next();
            if key_value.is_none() {
                break;
            }
            let kvb = key_value
                .unwrap() // safe to unwrap - just tested it
                .expect(EVENT_DESER_ERROR);
            batch.delete(kvb.0);
            snipped_count += 1;
        }

        // delete the key and reduce entry_count
        self.db.write(batch).expect(CRUD_ERROR);
        self.entry_count -= snipped_count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // third-party
    use more_asserts::assert_gt;
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    use serial_test::serial;
    use tempfile::TempDir;
    // internal
    use massa_models::config::THREAD_COUNT;
    use massa_models::output_event::EventExecutionContext;
    use massa_models::slot::Slot;

    fn setup() -> EventCache {
        let tmp_path = TempDir::new().unwrap().path().to_path_buf();
        EventCache::new(&tmp_path, 1000, 300, THREAD_COUNT)
    }

    #[test]
    #[serial]
    fn test_db_insert_order() {
        // Test that the data will be correctly ordered (when iterated from start) in db

        let mut cache = setup();
        let slot_1 = Slot::new(1, 0);
        let index_1_0 = 0;
        let event = SCOutputEvent {
            context: EventExecutionContext {
                slot: slot_1,
                block: None,
                read_only: false,
                index_in_slot: index_1_0,
                call_stack: Default::default(),
                origin_operation_id: None,
                is_final: true,
                is_error: false,
            },
            data: "message foo bar".to_string(),
        };

        let mut events = (0..cache.max_entry_count - 5)
            .map(|i| {
                let mut event = event.clone();
                event.context.index_in_slot = i as u64;
                event
            })
            .collect::<Vec<SCOutputEvent>>();

        let slot_2 = Slot::new(2, 0);
        let event_slot_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = 0u64;
            event
        };
        let index_2_2 = 256u64;
        let event_slot_2_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_2;
            event
        };
        events.push(event_slot_2.clone());
        events.push(event_slot_2_2.clone());
        // Randomize the events so we insert in random orders in DB
        events.shuffle(&mut thread_rng());

        for event in events {
            cache.insert(event);
        }

        // Now check that we are going to iter in correct order
        let db_it = cache.db_iter(Some(IteratorMode::Start));
        let mut prev_slot = None;
        let mut prev_event_index = None;
        #[allow(clippy::manual_flatten)]
        for kvb in db_it {
            if let Ok(kvb) = kvb {
                let bytes = kvb.0.iter().as_slice();
                let slot = Slot::from_bytes_key(&bytes[0..=8].try_into().unwrap());
                let event_index = u64::from_be_bytes(bytes[9..].try_into().unwrap());
                if prev_slot.is_some() && prev_event_index.is_some() {
                    assert_gt!(
                        (slot, event_index),
                        (prev_slot.unwrap(), prev_event_index.unwrap())
                    );
                } else {
                    assert_eq!(slot, slot_1);
                    assert_eq!(event_index, index_1_0);
                }
                prev_slot = Some(slot);
                prev_event_index = Some(event_index);
            }
        }

        assert_eq!(prev_slot, Some(slot_2));
        assert_eq!(prev_event_index, Some(index_2_2));
    }

    #[test]
    #[serial]
    fn test_insert_more_than_max_entry() {
        // Test insert (and snip) so we do not store too many event in the cache

        let mut cache = setup();
        let event = SCOutputEvent {
            context: EventExecutionContext {
                slot: Slot::new(1, 0),
                block: None,
                read_only: false,
                index_in_slot: 0,
                call_stack: Default::default(),
                origin_operation_id: None,
                is_final: true,
                is_error: false,
            },
            data: "message foo bar".to_string(),
        };

        // fill the db: add cache.max_entry_count entries
        for count in 0..cache.max_entry_count {
            let mut event = event.clone();
            event.context.index_in_slot = count as u64;
            cache.insert(event.clone());
        }
        assert_eq!(cache.entry_count, cache.max_entry_count);

        // insert one more entry
        let mut event_last = event.clone();
        event_last.context.index_in_slot = u64::MAX;
        cache.insert(event_last);
        assert_eq!(
            cache.entry_count,
            cache.max_entry_count - cache.snip_amount + 1
        );
        dbg!(cache.entry_count);
    }

    #[test]
    #[serial]
    fn test_insert_more_than_max_entry_2() {
        // Test insert_multi_it (and snip) so we do not store too many event in the cache

        let mut cache = setup();
        let event = SCOutputEvent {
            context: EventExecutionContext {
                slot: Slot::new(1, 0),
                block: None,
                read_only: false,
                index_in_slot: 0,
                call_stack: Default::default(),
                origin_operation_id: None,
                is_final: true,
                is_error: false,
            },
            data: "message foo bar".to_string(),
        };

        let it = (0..cache.max_entry_count).map(|i| {
            let mut event = event.clone();
            event.context.index_in_slot = i as u64;
            event
        });
        cache.insert_multi_it(it, Some(cache.max_entry_count));

        assert_eq!(cache.entry_count, cache.max_entry_count);

        // insert one more entry
        let mut event_last = event.clone();
        event_last.context.index_in_slot = u64::MAX;
        cache.insert(event_last);
        assert_eq!(
            cache.entry_count,
            cache.max_entry_count - cache.snip_amount + 1
        );
        dbg!(cache.entry_count);
    }

    #[test]
    #[serial]
    fn test_event_filter() {
        // Test that the data will be correctly ordered (when iterated from start) in db

        let mut cache = setup();
        let slot_1 = Slot::new(1, 0);
        let index_1_0 = 0;
        let event = SCOutputEvent {
            context: EventExecutionContext {
                slot: slot_1,
                block: None,
                read_only: false,
                index_in_slot: index_1_0,
                call_stack: Default::default(),
                origin_operation_id: None,
                is_final: true,
                is_error: false,
            },
            data: "message foo bar".to_string(),
        };

        let mut events = (0..cache.max_entry_count - 5)
            .map(|i| {
                let mut event = event.clone();
                event.context.index_in_slot = i as u64;
                event
            })
            .collect::<Vec<SCOutputEvent>>();

        let slot_2 = Slot::new(2, 0);
        let index_2_1 = 0u64;
        let event_slot_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_1;
            event
        };
        let index_2_2 = 256u64;
        let event_slot_2_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_2;
            event
        };
        events.push(event_slot_2.clone());
        events.push(event_slot_2_2.clone());
        // Randomize the events so we insert in random orders in DB
        events.shuffle(&mut thread_rng());

        let events_len = events.len();
        cache.insert_multi_it(events.into_iter(), Some(events_len));

        let filter_1 = EventFilter {
            start: Some(Slot::new(2, 0)),
            end: None,
            emitter_address: None,
            original_caller_address: None,
            original_operation_id: None,
            is_final: None,
            is_error: None,
        };
        let filtered_events_1 = cache
            .get_filtered_sc_output_events(&filter_1)
            .collect::<Vec<SCOutputEvent>>();

        assert_eq!(filtered_events_1.len(), 2);
        assert_eq!(filtered_events_1[0].context.slot, slot_2);
        assert_eq!(filtered_events_1[0].context.index_in_slot, index_2_1);
        assert_eq!(filtered_events_1[1].context.slot, slot_2);
        assert_eq!(filtered_events_1[1].context.index_in_slot, index_2_2);
    }
}
