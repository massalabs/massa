// std
use std::cmp::max;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
// third-party
use num_enum::IntoPrimitive;
use rocksdb::{IteratorMode, Options, WriteBatch, DB};
use tracing::{debug, warn};
// internal
use crate::rocksdb_operator::counter_merge;
use crate::ser_deser::{
    SCOutputEventDeserializer, SCOutputEventDeserializerArgs, SCOutputEventSerializer,
};
use massa_models::address::Address;
use massa_models::error::ModelsError;
use massa_models::execution::EventFilter;
use massa_models::operation::{OperationId, OperationIdSerializer};
use massa_models::output_event::SCOutputEvent;
use massa_models::slot::Slot;
use massa_serialization::{DeserializeError, Deserializer, Serializer};

const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
// const COUNTER_INIT_ERROR: &str = "critical: cannot init rocksdb counters";
const DESTROY_ERROR: &str = "critical: rocksdb delete operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const EVENT_DESER_ERROR: &str = "critical: event deserialization failed";
const OPERATION_ID_DESER_ERROR: &str = "critical: deserialization failed for op id in rocksdb";
const COUNTER_ERROR: &str = "critical: cannot get counter";
const COUNTER_KEY_CREATION_ERROR: &str = "critical: cannot create counter key";

#[allow(dead_code)]
/// Prefix u8 used to identify rocksdb keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, IntoPrimitive)]
#[repr(u8)]
enum KeyIndent {
    Counter = 0,
    Event,
    EmitterAddress,
    OriginalCallerAddress,
    OriginalOperationId,
    IsError,
    IsFinal,
}

/// Key for this type of data that we want to get
enum KeyBuilderType<'a> {
    Slot(&'a Slot),
    Event(&'a Slot, u64),
    Address(&'a Address),
    OperationId(&'a OperationId),
    Bool(bool),
    None,
}

enum KeyKind {
    Regular,
    Prefix,
    Counter,
}

struct DbKeyBuilder {
    /// Operation Id Serializer
    op_id_ser: OperationIdSerializer,
}

impl DbKeyBuilder {
    fn new() -> Self {
        Self {
            op_id_ser: OperationIdSerializer::new(),
        }
    }

    /// Low level key builder function
    /// There is no guarantees that the key will be unique
    /// Recommended to use high level function like:
    /// `key_from_event`, `prefix_key_from_indent`,
    /// `prefix_key_from_filter_item` or `counter_key_from_filter_item`
    fn key(&self, indent: &KeyIndent, key_type: KeyBuilderType, key_kind: &KeyKind) -> Vec<u8> {
        let mut key_base = if matches!(key_kind, KeyKind::Counter) {
            vec![u8::from(KeyIndent::Counter), u8::from(*indent)]
        } else {
            vec![u8::from(*indent)]
        };

        match key_type {
            KeyBuilderType::Slot(slot) => {
                key_base.extend(slot.to_bytes_key());
            }
            KeyBuilderType::Event(slot, index) => {
                key_base.extend(slot.to_bytes_key());
                key_base.extend(index.to_be_bytes());
            }
            KeyBuilderType::Address(addr) => {
                let addr_bytes = addr.to_prefixed_bytes();
                let addr_bytes_len = addr_bytes.len();
                key_base.extend(addr_bytes);
                key_base.push(addr_bytes_len as u8);
            }
            KeyBuilderType::OperationId(op_id) => {
                let mut buffer = Vec::new();
                self.op_id_ser
                    .serialize(op_id, &mut buffer)
                    .expect(OPERATION_ID_DESER_ERROR);
                key_base.extend(&buffer);
                key_base.extend(u32::to_be_bytes(buffer.len() as u32));
            }
            KeyBuilderType::Bool(value) => {
                key_base.push(u8::from(value));
            }
            KeyBuilderType::None => {}
        }

        key_base
    }

    /// Key usually used to populate the DB
    fn key_from_event(
        &self,
        event: &SCOutputEvent,
        indent: &KeyIndent,
        key_kind: &KeyKind,
    ) -> Option<Vec<u8>> {
        // Db format:
        // * Regular keys:
        //   * Event key: [Event Indent][Slot][Index] -> Event value: Event serialized
        //   * Emitter address key: [Emitter Address Indent][Addr][Addr len][Event key] -> []
        // * Prefix keys:
        //   * Emitter address prefix key: [Emitter Address Indent][Addr][Addr len]
        // * Counter keys:
        //   * Emitter address counter key: [Counter indent][Emitter Address Indent][Addr][Addr len][Event key] -> u64

        let key = match indent {
            KeyIndent::Event => {
                let item = KeyBuilderType::Event(&event.context.slot, event.context.index_in_slot);
                Some(self.key(indent, item, key_kind))
            }
            KeyIndent::EmitterAddress => {
                if let Some(addr) = event.context.call_stack.back() {
                    let item = KeyBuilderType::Address(addr);
                    let mut key = self.key(indent, item, key_kind);
                    let item =
                        KeyBuilderType::Event(&event.context.slot, event.context.index_in_slot);
                    if matches!(key_kind, KeyKind::Regular) {
                        key.extend(self.key(&KeyIndent::Event, item, &KeyKind::Regular));
                    }
                    Some(key)
                } else {
                    None
                }
            }
            KeyIndent::OriginalCallerAddress => {
                if let Some(addr) = event.context.call_stack.front() {
                    let item = KeyBuilderType::Address(addr);
                    let mut key = self.key(indent, item, key_kind);
                    let item =
                        KeyBuilderType::Event(&event.context.slot, event.context.index_in_slot);
                    if matches!(key_kind, KeyKind::Regular) {
                        key.extend(self.key(&KeyIndent::Event, item, &KeyKind::Regular));
                    }
                    Some(key)
                } else {
                    None
                }
            }
            KeyIndent::OriginalOperationId => {
                if let Some(op_id) = event.context.origin_operation_id.as_ref() {
                    let item = KeyBuilderType::OperationId(op_id);
                    let mut key = self.key(indent, item, key_kind);
                    let item =
                        KeyBuilderType::Event(&event.context.slot, event.context.index_in_slot);
                    if matches!(key_kind, KeyKind::Regular) {
                        key.extend(self.key(&KeyIndent::Event, item, &KeyKind::Regular));
                    }
                    Some(key)
                } else {
                    None
                }
            }
            KeyIndent::IsError => {
                let item = KeyBuilderType::Bool(event.context.is_error);
                let mut key = self.key(indent, item, key_kind);
                let item = KeyBuilderType::Event(&event.context.slot, event.context.index_in_slot);
                if matches!(key_kind, KeyKind::Regular) {
                    key.extend(self.key(&KeyIndent::Event, item, &KeyKind::Regular));
                }
                Some(key)
            }
            _ => unreachable!(),
        };

        key
    }

    /// Prefix key to iterate over all events / emitter_address / ...
    fn prefix_key_from_indent(&self, indent: &KeyIndent) -> Vec<u8> {
        self.key(indent, KeyBuilderType::None, &KeyKind::Regular)
    }

    /// Prefix key to iterate over specific emitter_address / operation_id / ...
    fn prefix_key_from_filter_item(&self, filter_item: &FilterItem, indent: &KeyIndent) -> Vec<u8> {
        match (indent, filter_item) {
            (KeyIndent::Event, FilterItem::SlotStartEnd(_start, _end)) => {
                unimplemented!()
            }
            (KeyIndent::Event, FilterItem::SlotStart(start)) => {
                self.key(indent, KeyBuilderType::Slot(start), &KeyKind::Prefix)
            }
            (KeyIndent::Event, FilterItem::SlotEnd(end)) => {
                self.key(indent, KeyBuilderType::Slot(end), &KeyKind::Prefix)
            }
            (KeyIndent::EmitterAddress, FilterItem::EmitterAddress(addr)) => {
                self.key(indent, KeyBuilderType::Address(addr), &KeyKind::Prefix)
            }
            (KeyIndent::OriginalCallerAddress, FilterItem::OriginalCallerAddress(addr)) => {
                self.key(indent, KeyBuilderType::Address(addr), &KeyKind::Prefix)
            }
            (KeyIndent::OriginalOperationId, FilterItem::OriginalOperationId(op_id)) => {
                self.key(indent, KeyBuilderType::OperationId(op_id), &KeyKind::Prefix)
            }
            (KeyIndent::IsError, FilterItem::IsError(v)) => {
                self.key(indent, KeyBuilderType::Bool(*v), &KeyKind::Prefix)
            }
            _ => {
                unreachable!()
            }
        }
    }

    /// Counter key for specific emitter_address / operation_id / ...
    fn counter_key_from_filter_item(
        &self,
        filter_item: &FilterItem,
        indent: &KeyIndent,
    ) -> Vec<u8> {
        // High level key builder function
        match (indent, filter_item) {
            (KeyIndent::Event, FilterItem::SlotStartEnd(_start, _end)) => {
                unimplemented!()
            }
            (KeyIndent::Event, FilterItem::SlotStart(start)) => {
                self.key(indent, KeyBuilderType::Slot(start), &KeyKind::Counter)
            }
            (KeyIndent::Event, FilterItem::SlotEnd(end)) => {
                self.key(indent, KeyBuilderType::Slot(end), &KeyKind::Counter)
            }
            (KeyIndent::EmitterAddress, FilterItem::EmitterAddress(addr)) => {
                self.key(indent, KeyBuilderType::Address(addr), &KeyKind::Counter)
            }
            (KeyIndent::OriginalCallerAddress, FilterItem::OriginalCallerAddress(addr)) => {
                self.key(indent, KeyBuilderType::Address(addr), &KeyKind::Counter)
            }
            (KeyIndent::OriginalOperationId, FilterItem::OriginalOperationId(op_id)) => self.key(
                indent,
                KeyBuilderType::OperationId(op_id),
                &KeyKind::Counter,
            ),
            (KeyIndent::IsError, FilterItem::IsError(v)) => {
                self.key(indent, KeyBuilderType::Bool(*v), &KeyKind::Counter)
            }
            _ => {
                unreachable!()
            }
        }
    }
}

/// Disk based event cache db (rocksdb based)
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
    /// Key builder
    key_builder: DbKeyBuilder,
    /// First event slot in db
    first_slot: Slot,
    /// Last event slot in db
    last_slot: Slot,
    /// Thread count
    thread_count: u8,
    /// Maximum number of events per operation
    max_events_per_operation: u64,
    /// Maximum number of operations per block
    max_operations_per_block: u64,
    /// Max number of events returned by a query
    max_events_per_query: usize,
}

impl EventCache {
    #[allow(clippy::too_many_arguments)]
    /// Create a new EventCache
    pub fn new(
        path: &Path,
        max_entry_count: usize,
        snip_amount: usize,
        thread_count: u8,
        max_recursive_call_depth: u16,
        max_event_data_length: u64,
        max_events_per_operation: u64,
        max_operations_per_block: u64,
        max_events_per_query: usize,
    ) -> Self {
        // Clear the db
        if path.exists() {
            DB::destroy(&Options::default(), path).expect(DESTROY_ERROR);
        }
        let options = {
            let mut opts = Options::default();
            opts.create_if_missing(true);
            opts.set_merge_operator_associative("counter merge operator", counter_merge);
            opts
        };
        let db = DB::open(&options, path).expect(OPEN_ERROR);

        let key_builder_2 = DbKeyBuilder::new();

        Self {
            db,
            entry_count: 0,
            max_entry_count,
            snip_amount,
            event_ser: SCOutputEventSerializer::new(),
            event_deser: SCOutputEventDeserializer::new(SCOutputEventDeserializerArgs {
                thread_count,
                max_call_stack_length: max_recursive_call_depth,
                max_event_data_length,
            }),
            key_builder: key_builder_2,
            first_slot: Slot::new(0, 0),
            last_slot: Slot::new(0, 0),
            thread_count,
            max_events_per_operation,
            max_operations_per_block,
            max_events_per_query,
        }
    }

    /// From an event add keys & values into a rocksdb batch
    fn insert_into_batch(&mut self, event: SCOutputEvent, batch: &mut WriteBatch) {
        let mut event_buffer = Vec::new();
        self.event_ser.serialize(&event, &mut event_buffer).unwrap();

        batch.put(
            self.key_builder
                .key_from_event(&event, &KeyIndent::Event, &KeyKind::Regular)
                .unwrap(),
            event_buffer,
        );

        if let Some(key) =
            self.key_builder
                .key_from_event(&event, &KeyIndent::EmitterAddress, &KeyKind::Regular)
        {
            let key_counter = self.key_builder.key_from_event(
                &event,
                &KeyIndent::EmitterAddress,
                &KeyKind::Counter,
            );
            batch.put(key, vec![]);
            let key_counter = key_counter.expect(COUNTER_KEY_CREATION_ERROR);
            batch.merge(key_counter, 1i64.to_be_bytes());
        }

        if let Some(key) = self.key_builder.key_from_event(
            &event,
            &KeyIndent::OriginalCallerAddress,
            &KeyKind::Regular,
        ) {
            let key_counter = self.key_builder.key_from_event(
                &event,
                &KeyIndent::OriginalCallerAddress,
                &KeyKind::Counter,
            );
            batch.put(key, vec![]);
            let key_counter = key_counter.expect(COUNTER_KEY_CREATION_ERROR);
            batch.merge(key_counter, 1i64.to_be_bytes());
        }

        if let Some(key) = self.key_builder.key_from_event(
            &event,
            &KeyIndent::OriginalOperationId,
            &KeyKind::Regular,
        ) {
            let key_counter = self.key_builder.key_from_event(
                &event,
                &KeyIndent::OriginalOperationId,
                &KeyKind::Counter,
            );
            batch.put(key, vec![]);
            let key_counter = key_counter.expect(COUNTER_KEY_CREATION_ERROR);
            batch.merge(key_counter, 1i64.to_be_bytes());
        }

        {
            if let Some(key) =
                self.key_builder
                    .key_from_event(&event, &KeyIndent::IsError, &KeyKind::Regular)
            {
                let key_counter =
                    self.key_builder
                        .key_from_event(&event, &KeyIndent::IsError, &KeyKind::Counter);
                let key_counter = key_counter.expect(COUNTER_KEY_CREATION_ERROR);
                batch.put(key, vec![]);
                batch.merge(key_counter, 1i64.to_be_bytes());
            }
        }

        // Keep track of last slot (and start slot) of events in the DB
        // Help for event filtering
        self.last_slot = max(self.last_slot, event.context.slot);
    }

    #[allow(dead_code)]
    /// Insert a new event in the cache
    pub fn insert(&mut self, event: SCOutputEvent) {
        if self.entry_count >= self.max_entry_count {
            self.snip(None);
        }

        let mut batch = WriteBatch::default();
        self.insert_into_batch(event, &mut batch);
        self.db.write(batch).expect(CRUD_ERROR);

        // Note:
        // This assumes that events are always added, never overwritten
        self.entry_count = self.entry_count.saturating_add(1);

        debug!("(Event insert) entry_count is: {}", self.entry_count);
    }

    /// Insert new events in the cache
    pub fn insert_multi_it(
        &mut self,
        events: impl ExactSizeIterator<Item = SCOutputEvent> + Clone,
    ) {
        let events_len = events.len();

        if self.entry_count + events_len >= self.max_entry_count {
            let snip_amount = max(self.snip_amount, events_len);
            self.snip(Some(snip_amount));
        }

        let mut batch = WriteBatch::default();
        for event in events {
            self.insert_into_batch(event, &mut batch);
        }
        self.db.write(batch).expect(CRUD_ERROR);
        // Note:
        // This assumes that events are always added, never overwritten
        self.entry_count = self.entry_count.saturating_add(events_len);

        debug!("(Events insert) entry_count is: {}", self.entry_count);
    }

    /// Get events filtered by the given argument
    pub(crate) fn get_filtered_sc_output_events(
        &self,
        filter: &EventFilter,
    ) -> (Vec<u64>, Vec<SCOutputEvent>) {
        // Step 1
        // Build a (sorted) map with key: (counter value, indent), value: filter
        // Will be used to iterate from the lower count index to the highest count index
        // e.g. if index for emitter address is 10 (index count), and origin operation id is 20
        //      iter over emitter address index then origin operation id index

        let filter_items = from_event_filter(filter);

        if filter_items.is_empty() {
            // Note: will return too many event - user should restrict the filter
            warn!("Filter item only on is final field, please add more filter parameters");
            return (vec![], vec![]);
        }

        let it = filter_items.iter().map(|(key_indent, filter_item)| {
            let count = self
                .filter_item_estimate_count(key_indent, filter_item)
                .unwrap_or_else(|e| {
                    warn!(
                        "Could not estimate count for key indent: {:?} - filter_item: {:?}: {}",
                        key_indent, filter_item, e
                    );
                    self.max_entry_count as u64
                });
            ((count, key_indent), filter_item)
        });

        let map = BTreeMap::from_iter(it);
        debug!("Filter items map: {:?}", map);

        // Step 2: apply filter from the lowest counter to the highest counter

        let mut query_counts = Vec::with_capacity(map.len());
        let mut filter_res_prev = None;
        for ((_counter, indent), filter_item) in map.iter() {
            let mut filter_res = BTreeSet::new();
            let query_count = self.filter_for(
                indent,
                filter_item,
                &mut filter_res,
                filter_res_prev.as_ref(),
            );
            query_counts.push(query_count);
            filter_res_prev = Some(filter_res);
        }

        // Step 3: get values & deserialize

        let multi_args = filter_res_prev
            .unwrap()
            .into_iter()
            .take(self.max_events_per_query)
            .collect::<Vec<Vec<u8>>>();

        let res = self.db.multi_get(multi_args);
        debug!(
            "Filter will try to deserialize to SCOutputEvent {} values",
            res.len()
        );

        let events = res
            .into_iter()
            .map(|value| {
                let value = value.unwrap().unwrap();
                let (_, event) = self
                    .event_deser
                    .deserialize::<DeserializeError>(&value)
                    .unwrap();
                event
            })
            .collect::<Vec<SCOutputEvent>>();

        (query_counts, events)
    }

    fn filter_for(
        &self,
        indent: &KeyIndent,
        filter_item: &FilterItem,
        result: &mut BTreeSet<Vec<u8>>,
        seen: Option<&BTreeSet<Vec<u8>>>,
    ) -> u64 {
        let mut query_count: u64 = 0;

        if *indent == KeyIndent::Event {
            let opts = match filter_item {
                FilterItem::SlotStart(_start) => {
                    let key_start = self
                        .key_builder
                        .prefix_key_from_filter_item(filter_item, indent);
                    let mut options = rocksdb::ReadOptions::default();
                    options.set_iterate_lower_bound(key_start);
                    options
                }
                FilterItem::SlotEnd(_end) => {
                    let key_end = self
                        .key_builder
                        .prefix_key_from_filter_item(filter_item, indent);
                    let mut options = rocksdb::ReadOptions::default();
                    options.set_iterate_upper_bound(key_end);
                    options
                }
                FilterItem::SlotStartEnd(start, end) => {
                    let key_start = self
                        .key_builder
                        .prefix_key_from_filter_item(&FilterItem::SlotStart(*start), indent);
                    let key_end = self
                        .key_builder
                        .prefix_key_from_filter_item(&FilterItem::SlotEnd(*end), indent);
                    let mut options = rocksdb::ReadOptions::default();
                    options.set_iterate_range(key_start..key_end);
                    options
                }
                _ => unreachable!(),
            };

            #[allow(clippy::manual_flatten)]
            for kvb in self.db.iterator_opt(IteratorMode::Start, opts) {
                if let Ok(kvb) = kvb {
                    if !kvb.0.starts_with(&[*indent as u8]) {
                        // Stop as soon as our key does not start with the right indent
                        break;
                    }

                    let found = kvb.0.to_vec();
                    query_count = query_count.saturating_add(1);

                    if let Some(filter_set_seen) = seen {
                        if filter_set_seen.contains(&found) {
                            result.insert(found);
                        }

                        // We have already found as many items as in a previous search
                        // As we search from the lowest count to the highest count, we will never add new items
                        // in our result, so we can break here
                        if filter_set_seen.len() == result.len() {
                            break;
                        }
                    } else {
                        result.insert(found);
                    }
                }
            }
        } else {
            let prefix_filter = match filter_item {
                FilterItem::EmitterAddress(_addr) => self
                    .key_builder
                    .prefix_key_from_filter_item(filter_item, indent),
                FilterItem::OriginalCallerAddress(_addr) => self
                    .key_builder
                    .prefix_key_from_filter_item(filter_item, indent),
                FilterItem::OriginalOperationId(_op_id) => self
                    .key_builder
                    .prefix_key_from_filter_item(filter_item, indent),
                FilterItem::IsError(_is_error) => self
                    .key_builder
                    .prefix_key_from_filter_item(filter_item, indent),
                _ => unreachable!(),
            };

            #[allow(clippy::manual_flatten)]
            for kvb in self.db.prefix_iterator(prefix_filter.as_slice()) {
                if let Ok(kvb) = kvb {
                    if !kvb.0.starts_with(&[*indent as u8]) {
                        // Stop as soon as our key does not start with the right indent
                        break;
                    }

                    if !kvb.0.starts_with(prefix_filter.as_slice()) {
                        // Stop as soon as our key does not start with our current prefix
                        break;
                    }

                    let found = kvb
                        .0
                        .strip_prefix(prefix_filter.as_slice())
                        .unwrap() // safe to unwrap() - already tested
                        .to_vec();

                    query_count = query_count.saturating_add(1);

                    if let Some(filter_set_seen) = seen {
                        if filter_set_seen.contains(&found) {
                            result.insert(found);
                        }

                        // We have already found as many items as in a previous search
                        // As we search from the lowest count to the highest count, we will never add new items
                        // in our result, so we can break here
                        if filter_set_seen.len() == result.len() {
                            break;
                        }
                    } else {
                        result.insert(found);
                    }
                }
            }
        }

        query_count
    }

    /// Estimate for a given KeyIndent & FilterItem the number of row to iterate
    fn filter_item_estimate_count(
        &self,
        key_indent: &KeyIndent,
        filter_item: &FilterItem,
    ) -> Result<u64, ModelsError> {
        match filter_item {
            FilterItem::SlotStart(start) => {
                let diff = self.last_slot.slots_since(start, self.thread_count)?;
                // Note: Pessimistic estimation - should we keep an average count of events per slot
                //       and use that instead?
                Ok(diff
                    .saturating_mul(self.max_events_per_operation)
                    .saturating_mul(self.max_operations_per_block))
            }
            FilterItem::SlotStartEnd(start, end) => {
                let diff = end.slots_since(start, self.thread_count)?;
                Ok(diff
                    .saturating_mul(self.max_events_per_operation)
                    .saturating_mul(self.max_operations_per_block))
            }
            FilterItem::SlotEnd(end) => {
                let diff = end.slots_since(&self.first_slot, self.thread_count)?;
                Ok(diff
                    .saturating_mul(self.max_events_per_operation)
                    .saturating_mul(self.max_operations_per_block))
            }
            FilterItem::EmitterAddress(_addr) => {
                let counter_key = self
                    .key_builder
                    .counter_key_from_filter_item(filter_item, key_indent);
                let counter = self.db.get(counter_key).expect(COUNTER_ERROR);
                let counter_value = counter
                    .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
                    .unwrap_or(0);
                Ok(counter_value)
            }
            FilterItem::OriginalCallerAddress(_addr) => {
                let counter_key = self
                    .key_builder
                    .counter_key_from_filter_item(filter_item, key_indent);
                let counter = self.db.get(counter_key).expect(COUNTER_ERROR);
                let counter_value = counter
                    .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
                    .unwrap_or(0);
                Ok(counter_value)
            }
            FilterItem::OriginalOperationId(_op_id) => {
                let counter_key = self
                    .key_builder
                    .counter_key_from_filter_item(filter_item, key_indent);
                let counter = self.db.get(counter_key).expect(COUNTER_ERROR);
                let counter_value = counter
                    .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
                    .unwrap_or(0);
                Ok(counter_value)
            }
            FilterItem::IsError(_is_error) => {
                let counter_key = self
                    .key_builder
                    .counter_key_from_filter_item(filter_item, key_indent);
                let counter = self.db.get(counter_key).expect(COUNTER_ERROR);
                let counter_value = counter
                    .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
                    .unwrap_or(0);
                Ok(counter_value)
            }
        }
    }

    /// Try to remove some entries from the db
    fn snip(&mut self, snip_amount: Option<usize>) {
        let mut iter = self.db.iterator(IteratorMode::Start);
        let mut batch = WriteBatch::default();
        let mut snipped_count: usize = 0;
        let snip_amount = snip_amount.unwrap_or(self.snip_amount);

        let mut counter_keys = vec![];

        while snipped_count < snip_amount {
            let key_value = iter.next();
            if key_value.is_none() {
                break;
            }
            let kvb = key_value
                .unwrap() // safe to unwrap - just tested it
                .expect(EVENT_DESER_ERROR);

            let key = kvb.0;
            if !key.starts_with(&[u8::from(KeyIndent::Event)]) {
                continue;
            }

            let (_, event) = self
                .event_deser
                .deserialize::<DeserializeError>(&kvb.1)
                .unwrap();

            // delete all associated key
            if let Some(key) = self.key_builder.key_from_event(
                &event,
                &KeyIndent::EmitterAddress,
                &KeyKind::Regular,
            ) {
                let key_counter = self
                    .key_builder
                    .key_from_event(&event, &KeyIndent::EmitterAddress, &KeyKind::Counter)
                    .expect(COUNTER_ERROR);
                batch.delete(key);
                counter_keys.push(key_counter.clone());
                batch.merge(key_counter, (-1i64).to_be_bytes());
            }
            if let Some(key) = self.key_builder.key_from_event(
                &event,
                &KeyIndent::OriginalCallerAddress,
                &KeyKind::Regular,
            ) {
                let key_counter = self
                    .key_builder
                    .key_from_event(&event, &KeyIndent::OriginalCallerAddress, &KeyKind::Counter)
                    .expect(COUNTER_ERROR);
                batch.delete(key);
                counter_keys.push(key_counter.clone());
                batch.merge(key_counter, (-1i64).to_be_bytes());
            }

            if let Some(key) = self.key_builder.key_from_event(
                &event,
                &KeyIndent::OriginalOperationId,
                &KeyKind::Regular,
            ) {
                let key_counter = self
                    .key_builder
                    .key_from_event(&event, &KeyIndent::OriginalOperationId, &KeyKind::Counter)
                    .expect(COUNTER_ERROR);
                batch.delete(key);
                counter_keys.push(key_counter.clone());
                batch.merge(key_counter, (-1i64).to_be_bytes());
            }
            if let Some(key) =
                self.key_builder
                    .key_from_event(&event, &KeyIndent::IsError, &KeyKind::Regular)
            {
                let key_counter = self
                    .key_builder
                    .key_from_event(&event, &KeyIndent::IsError, &KeyKind::Counter)
                    .expect(COUNTER_ERROR);
                batch.delete(key);
                counter_keys.push(key_counter.clone());
                batch.merge(key_counter, (-1i64).to_be_bytes());
            }

            batch.delete(key);
            snipped_count += 1;
        }

        // delete the key and reduce entry_count
        self.db.write(batch).expect(CRUD_ERROR);
        self.entry_count = self.entry_count.saturating_sub(snipped_count);

        // delete key counters where value == 0
        let mut batch_counters = WriteBatch::default();
        const U64_ZERO_BYTES: [u8; 8] = 0u64.to_be_bytes();
        for (value, key) in self.db.multi_get(&counter_keys).iter().zip(counter_keys) {
            if let Ok(Some(value)) = value {
                if *value == U64_ZERO_BYTES.to_vec() {
                    batch_counters.delete(key);
                }
            }
        }
        self.db.write(batch_counters).expect(CRUD_ERROR);

        // Update first_slot / last_slot in the DB
        if self.entry_count == 0 {
            // Reset
            self.first_slot = Slot::new(0, 0);
            self.last_slot = Slot::new(0, 0);
        } else {
            // Get the first event in the db
            // By using a prefix iterator this should be fast

            let key_prefix = self.key_builder.prefix_key_from_indent(&KeyIndent::Event);
            let mut it_slot = self.db.prefix_iterator(key_prefix);

            let key_value = it_slot.next();
            let kvb = key_value.unwrap().expect(EVENT_DESER_ERROR);

            let (_, event) = self
                .event_deser
                .deserialize::<DeserializeError>(&kvb.1)
                .unwrap();
            self.first_slot = event.context.slot;
        }
    }
}

/// A filter parameter - used to decompose an EventFilter in multiple filters
#[derive(Debug)]
enum FilterItem {
    SlotStart(Slot),
    SlotStartEnd(Slot, Slot),
    SlotEnd(Slot),
    EmitterAddress(Address),
    OriginalCallerAddress(Address),
    OriginalOperationId(OperationId),
    IsError(bool),
}

/// Convert a EventFilter into a list of (KeyIndent, FilterItem)
fn from_event_filter(event_filter: &EventFilter) -> Vec<(KeyIndent, FilterItem)> {
    let mut filter_items = vec![];
    if event_filter.start.is_some() && event_filter.end.is_some() {
        let start = event_filter.start.unwrap();
        let end = event_filter.end.unwrap();
        filter_items.push((KeyIndent::Event, FilterItem::SlotStartEnd(start, end)));
    } else if event_filter.start.is_some() {
        let start = event_filter.start.unwrap();
        filter_items.push((KeyIndent::Event, FilterItem::SlotStart(start)));
    } else if event_filter.end.is_some() {
        let end = event_filter.end.unwrap();
        filter_items.push((KeyIndent::Event, FilterItem::SlotEnd(end)));
    }

    if let Some(addr) = event_filter.emitter_address {
        filter_items.push((KeyIndent::EmitterAddress, FilterItem::EmitterAddress(addr)));
    }

    if let Some(addr) = event_filter.original_caller_address {
        filter_items.push((
            KeyIndent::OriginalCallerAddress,
            FilterItem::OriginalCallerAddress(addr),
        ));
    }

    if let Some(op_id) = event_filter.original_operation_id {
        filter_items.push((
            KeyIndent::OriginalOperationId,
            FilterItem::OriginalOperationId(op_id),
        ));
    }

    if let Some(is_error) = event_filter.is_error {
        filter_items.push((KeyIndent::IsError, FilterItem::IsError(is_error)));
    }

    filter_items
}

#[cfg(test)]
impl EventCache {
    /// Iterate over all keys & values in the db - test only
    fn iter_all(
        &self,
        mode: Option<IteratorMode>,
    ) -> impl Iterator<Item = (Box<[u8]>, Box<[u8]>)> + '_ {
        self.db
            .iterator(mode.unwrap_or(IteratorMode::Start))
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // std
    use std::collections::VecDeque;
    use std::str::FromStr;
    // third-party
    use more_asserts::assert_gt;
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    use serial_test::serial;
    use tempfile::TempDir;
    // internal
    use massa_models::config::{
        MAX_EVENT_DATA_SIZE_V1, MAX_EVENT_PER_OPERATION, MAX_OPERATIONS_PER_BLOCK,
        MAX_RECURSIVE_CALLS_DEPTH, THREAD_COUNT,
    };
    use massa_models::operation::OperationId;
    use massa_models::output_event::EventExecutionContext;
    use massa_models::slot::Slot;

    fn setup() -> EventCache {
        let tmp_path = TempDir::new().unwrap().path().to_path_buf();
        EventCache::new(
            &tmp_path,
            1000,
            300,
            THREAD_COUNT,
            MAX_RECURSIVE_CALLS_DEPTH,
            MAX_EVENT_DATA_SIZE_V1 as u64,
            MAX_EVENT_PER_OPERATION as u64,
            MAX_OPERATIONS_PER_BLOCK as u64,
            5000, // MAX_EVENTS_PER_QUERY,
        )
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

        let max_entry_count = cache.max_entry_count - 5;
        let mut events = (0..max_entry_count)
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
        // let db_it = cache.db_iter(Some(IteratorMode::Start));
        let mut prev_slot = None;
        let mut prev_event_index = None;
        for kvb in cache.iter_all(None) {
            let bytes = kvb.0.iter().as_slice();

            if bytes[0] != u8::from(KeyIndent::Event) {
                continue;
            }

            let slot = Slot::from_bytes_key(&bytes[1..=9].try_into().unwrap());
            let event_index = u64::from_be_bytes(bytes[10..].try_into().unwrap());
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
        cache.insert_multi_it(it);

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
    fn test_snip() {
        // Test snip so we enforce that all db keys are removed

        let mut cache = setup();
        cache.max_entry_count = 10;

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
        cache.insert_multi_it(it);

        assert_eq!(cache.entry_count, cache.max_entry_count);

        cache.snip(Some(cache.entry_count));

        assert_eq!(cache.entry_count, 0);
        assert_eq!(cache.iter_all(None).count(), 0);
    }

    #[test]
    #[serial]
    fn test_counter_0() {
        // Test snip so we enforce that all db keys are removed

        let mut cache = setup();
        cache.max_entry_count = 10;

        let dummy_addr =
            Address::from_str("AU12qePoXhNbYWE1jZuafqJong7bbq1jw3k89RgbMawbrdZpaasoA").unwrap();
        let emit_addr_1 =
            Address::from_str("AU122Em8qkqegdLb1eyH8rdkSCNEf7RZLeTJve4Q2inRPGiTJ2xNv").unwrap();
        let emit_addr_2 =
            Address::from_str("AU12WuVR1Td74q9eAbtYZUnk5jnRbUuUacyhQFwm217bV5v1mNqTZ").unwrap();

        let event = SCOutputEvent {
            context: EventExecutionContext {
                slot: Slot::new(1, 0),
                block: None,
                read_only: false,
                index_in_slot: 0,
                call_stack: VecDeque::from(vec![dummy_addr, emit_addr_1]),
                origin_operation_id: None,
                is_final: true,
                is_error: false,
            },
            data: "message foo bar".to_string(),
        };

        let event_2 = {
            let mut evt = event.clone();
            evt.context.slot = Slot::new(2, 0);
            evt.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
            evt
        };

        cache.insert_multi_it([event, event_2].into_iter());

        // Check counters key length
        let key_counters = cache
            .key_builder
            .prefix_key_from_indent(&KeyIndent::Counter);
        let kvbs: Result<Vec<(_, _)>, _> = cache
            .db
            .prefix_iterator(key_counters)
            .take_while(|kvb| {
                kvb.as_ref()
                    .unwrap()
                    .0
                    .starts_with(&[u8::from(KeyIndent::Counter)])
            })
            .collect();
        // println!("kvbs: {:#?}", kvbs);

        // Expected 4 counters:
        // 2 for emitter address (emit_addr_1 & emit_addr_2)
        // 1 for original caller address (dummy_addr)
        // 1 for is_error(false)
        assert_eq!(kvbs.unwrap().len(), 4);

        let key_counter_1 = cache.key_builder.counter_key_from_filter_item(
            &FilterItem::EmitterAddress(emit_addr_1),
            &KeyIndent::EmitterAddress,
        );
        let key_counter_2 = cache.key_builder.counter_key_from_filter_item(
            &FilterItem::EmitterAddress(emit_addr_2),
            &KeyIndent::EmitterAddress,
        );

        let v1 = cache.db.get(key_counter_1.clone());
        let v2 = cache.db.get(key_counter_2.clone());

        // println!("v1: {:?} - v2: {:?}", v1, v2);
        assert_eq!(v1, Ok(Some(1u64.to_be_bytes().to_vec())));
        assert_eq!(v2, Ok(Some(1u64.to_be_bytes().to_vec())));

        cache.snip(Some(1));

        let v1 = cache.db.get(key_counter_1);
        let v2 = cache.db.get(key_counter_2);

        // println!("v1: {:?} - v2: {:?}", v1, v2);
        assert_eq!(v1, Ok(None)); // counter has been removed
        assert_eq!(v2, Ok(Some(1u64.to_be_bytes().to_vec())));
    }

    #[test]
    #[serial]
    fn test_event_filter() {
        // Test that the data will be correctly ordered (when filtered) in db

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
        // Randomize the events so we insert in random orders in the DB
        events.shuffle(&mut thread_rng());

        cache.insert_multi_it(events.into_iter());

        let filter_1 = EventFilter {
            start: Some(Slot::new(2, 0)),
            ..Default::default()
        };

        let (_, filtered_events_1) = cache.get_filtered_sc_output_events(&filter_1);

        assert_eq!(filtered_events_1.len(), 2);
        assert_eq!(filtered_events_1[0].context.slot, slot_2);
        assert_eq!(filtered_events_1[0].context.index_in_slot, index_2_1);
        assert_eq!(filtered_events_1[1].context.slot, slot_2);
        assert_eq!(filtered_events_1[1].context.index_in_slot, index_2_2);
    }

    #[test]
    #[serial]
    fn test_event_filter_2() {
        // Test get_filtered_sc_output_events + op id

        let mut cache = setup();
        cache.max_entry_count = 10;

        let slot_1 = Slot::new(1, 0);
        let index_1_0 = 0;
        let op_id_1 =
            OperationId::from_str("O12n1vt8uTLh3H65J4TVuztaWfBh3oumjjVtRCkke7Ba5qWdXdjD").unwrap();
        let op_id_2 =
            OperationId::from_str("O1p5P691KF672fQ8tQougxzSERBwDKZF8FwtkifMSJbP14sEuGc").unwrap();
        let op_id_unknown =
            OperationId::from_str("O1kvXTfsnVbQcmDERkC89vqAd2xRTLCb3q5b2E5WaVPHwFd7Qth").unwrap();

        let event = SCOutputEvent {
            context: EventExecutionContext {
                slot: slot_1,
                block: None,
                read_only: false,
                index_in_slot: index_1_0,
                call_stack: Default::default(),
                origin_operation_id: Some(op_id_1),
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
            event.context.origin_operation_id = Some(op_id_2);
            event
        };
        let index_2_2 = 256u64;
        let event_slot_2_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_2;
            event.context.origin_operation_id = Some(op_id_2);
            event
        };
        events.push(event_slot_2.clone());
        events.push(event_slot_2_2.clone());
        // Randomize the events so we insert in random orders in the DB
        events.shuffle(&mut thread_rng());
        cache.insert_multi_it(events.into_iter());

        let mut filter_1 = EventFilter {
            original_operation_id: Some(op_id_1),
            ..Default::default()
        };

        let (_, filtered_events_1) = cache.get_filtered_sc_output_events(&filter_1);

        assert_eq!(filtered_events_1.len(), cache.max_entry_count - 5);
        filtered_events_1.iter().enumerate().for_each(|(i, event)| {
            assert_eq!(event.context.slot, slot_1);
            assert_eq!(event.context.index_in_slot, i as u64);
        });

        {
            filter_1.original_operation_id = Some(op_id_2);
            let (_, filtered_events_2) = cache.get_filtered_sc_output_events(&filter_1);
            assert_eq!(filtered_events_2.len(), 2);
            filtered_events_2.iter().enumerate().for_each(|(i, event)| {
                assert_eq!(event.context.slot, slot_2);
                if i == 0 {
                    assert_eq!(event.context.index_in_slot, i as u64);
                } else {
                    assert_eq!(event.context.index_in_slot, 256u64);
                }
            });
        }

        {
            filter_1.original_operation_id = Some(op_id_unknown);
            let (_, filtered_events_2) = cache.get_filtered_sc_output_events(&filter_1);
            assert_eq!(filtered_events_2.len(), 0);
        }
    }

    #[test]
    #[serial]
    fn test_event_filter_3() {
        // Test get_filtered_sc_output_events + emitter address

        let mut cache = setup();
        cache.max_entry_count = 10;

        let slot_1 = Slot::new(1, 0);
        let index_1_0 = 0;

        let dummy_addr =
            Address::from_str("AU12qePoXhNbYWE1jZuafqJong7bbq1jw3k89RgbMawbrdZpaasoA").unwrap();
        let emit_addr_1 =
            Address::from_str("AU122Em8qkqegdLb1eyH8rdkSCNEf7RZLeTJve4Q2inRPGiTJ2xNv").unwrap();
        let emit_addr_2 =
            Address::from_str("AU12WuVR1Td74q9eAbtYZUnk5jnRbUuUacyhQFwm217bV5v1mNqTZ").unwrap();
        let emit_addr_unknown =
            Address::from_str("AU1zLC4TFUiaKDg7quQyusMPQcHT4ykWVs3FsFpuhdNSmowUG2As").unwrap();

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

        let to_insert_count = cache.max_entry_count - 5;
        let threshold = to_insert_count / 2;
        let mut events = (0..cache.max_entry_count - 5)
            .map(|i| {
                let mut event = event.clone();
                event.context.index_in_slot = i as u64;
                if i < threshold {
                    event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_1]);
                } else {
                    event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
                }
                event
            })
            .collect::<Vec<SCOutputEvent>>();

        let slot_2 = Slot::new(2, 0);
        let index_2_1 = 0u64;
        let event_slot_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_1;
            event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
            event
        };
        let index_2_2 = 256u64;
        let event_slot_2_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_2;
            event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
            event
        };
        events.push(event_slot_2.clone());
        events.push(event_slot_2_2.clone());
        // Randomize the events so we insert in random orders in the DB
        events.shuffle(&mut thread_rng());

        cache.insert_multi_it(events.into_iter());

        let mut filter_1 = EventFilter {
            emitter_address: Some(emit_addr_1),
            ..Default::default()
        };

        let (_, filtered_events_1) = cache.get_filtered_sc_output_events(&filter_1);

        assert_eq!(filtered_events_1.len(), threshold);
        filtered_events_1.iter().for_each(|event| {
            assert_eq!(event.context.slot, slot_1);
            assert_eq!(*event.context.call_stack.back().unwrap(), emit_addr_1)
        });

        // filter with emit_addr_2
        {
            filter_1.emitter_address = Some(emit_addr_2);
            let (_, filtered_events_2) = cache.get_filtered_sc_output_events(&filter_1);
            assert_eq!(filtered_events_2.len(), threshold + 1 + 2);
            filtered_events_2.iter().for_each(|event| {
                assert_eq!(*event.context.call_stack.back().unwrap(), emit_addr_2)
            });
        }
        // filter with dummy_addr
        {
            filter_1.emitter_address = Some(dummy_addr);
            let (_, filtered_events_2) = cache.get_filtered_sc_output_events(&filter_1);
            assert_eq!(filtered_events_2.len(), 0);
        }
        // filter with address that is not in the DB
        {
            filter_1.emitter_address = Some(emit_addr_unknown);
            let (_, filtered_events_2) = cache.get_filtered_sc_output_events(&filter_1);
            assert_eq!(filtered_events_2.len(), 0);
        }
    }

    #[test]
    #[serial]
    fn test_event_filter_4() {
        // Test get_filtered_sc_output_events + original caller addr

        let mut cache = setup();
        cache.max_entry_count = 10;

        let slot_1 = Slot::new(1, 0);
        let index_1_0 = 0;

        let dummy_addr =
            Address::from_str("AU12qePoXhNbYWE1jZuafqJong7bbq1jw3k89RgbMawbrdZpaasoA").unwrap();
        let caller_addr_1 =
            Address::from_str("AU122Em8qkqegdLb1eyH8rdkSCNEf7RZLeTJve4Q2inRPGiTJ2xNv").unwrap();
        let caller_addr_2 =
            Address::from_str("AU12WuVR1Td74q9eAbtYZUnk5jnRbUuUacyhQFwm217bV5v1mNqTZ").unwrap();
        let caller_addr_unknown =
            Address::from_str("AU1zLC4TFUiaKDg7quQyusMPQcHT4ykWVs3FsFpuhdNSmowUG2As").unwrap();

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

        let to_insert_count = cache.max_entry_count - 5;
        let threshold = to_insert_count / 2;
        let mut events = (0..cache.max_entry_count - 5)
            .map(|i| {
                let mut event = event.clone();
                event.context.index_in_slot = i as u64;
                if i < threshold {
                    event.context.call_stack = VecDeque::from(vec![caller_addr_1, dummy_addr]);
                } else {
                    event.context.call_stack = VecDeque::from(vec![caller_addr_2, dummy_addr]);
                }
                event
            })
            .collect::<Vec<SCOutputEvent>>();

        let slot_2 = Slot::new(2, 0);
        let index_2_1 = 0u64;
        let event_slot_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_1;
            event.context.call_stack = VecDeque::from(vec![caller_addr_2, dummy_addr]);
            event
        };
        let index_2_2 = 256u64;
        let event_slot_2_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_2;
            event.context.call_stack = VecDeque::from(vec![caller_addr_2, dummy_addr]);
            event
        };
        events.push(event_slot_2.clone());
        events.push(event_slot_2_2.clone());
        // Randomize the events so we insert in random orders in the DB
        events.shuffle(&mut thread_rng());
        cache.insert_multi_it(events.into_iter());

        let mut filter_1 = EventFilter {
            original_caller_address: Some(caller_addr_1),
            ..Default::default()
        };

        let (_, filtered_events_1) = cache.get_filtered_sc_output_events(&filter_1);

        assert_eq!(filtered_events_1.len(), threshold);
        filtered_events_1.iter().for_each(|event| {
            assert_eq!(event.context.slot, slot_1);
            assert_eq!(*event.context.call_stack.front().unwrap(), caller_addr_1);
        });

        {
            filter_1.original_caller_address = Some(caller_addr_2);
            let (_, filtered_events_2) = cache.get_filtered_sc_output_events(&filter_1);
            assert_eq!(filtered_events_2.len(), threshold + 1 + 2);
            filtered_events_2.iter().for_each(|event| {
                assert_eq!(*event.context.call_stack.front().unwrap(), caller_addr_2);
            });
        }
        {
            filter_1.original_caller_address = Some(dummy_addr);
            let (_, filtered_events_2) = cache.get_filtered_sc_output_events(&filter_1);
            assert_eq!(filtered_events_2.len(), 0);
        }
        {
            filter_1.original_caller_address = Some(caller_addr_unknown);
            let (_, filtered_events_2) = cache.get_filtered_sc_output_events(&filter_1);
            assert_eq!(filtered_events_2.len(), 0);
        }
    }

    #[test]
    #[serial]
    fn test_event_filter_5() {
        // Test get_filtered_sc_output_events + is error

        let mut cache = setup();
        cache.max_entry_count = 10;

        let slot_1 = Slot::new(1, 0);
        let index_1_0 = 0;

        let dummy_addr =
            Address::from_str("AU12qePoXhNbYWE1jZuafqJong7bbq1jw3k89RgbMawbrdZpaasoA").unwrap();
        let emit_addr_1 =
            Address::from_str("AU122Em8qkqegdLb1eyH8rdkSCNEf7RZLeTJve4Q2inRPGiTJ2xNv").unwrap();
        let emit_addr_2 =
            Address::from_str("AU12WuVR1Td74q9eAbtYZUnk5jnRbUuUacyhQFwm217bV5v1mNqTZ").unwrap();

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

        let to_insert_count = cache.max_entry_count - 5;
        let threshold = to_insert_count / 2;
        let mut events = (0..cache.max_entry_count - 5)
            .map(|i| {
                let mut event = event.clone();
                event.context.index_in_slot = i as u64;
                if i < threshold {
                    event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_1]);
                } else {
                    event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
                }
                event
            })
            .collect::<Vec<SCOutputEvent>>();

        let slot_2 = Slot::new(2, 0);
        let index_2_1 = 0u64;
        let event_slot_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_1;
            event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
            event
        };
        let index_2_2 = 256u64;
        let event_slot_2_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_2;
            event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
            event.context.is_error = true;
            event
        };
        events.push(event_slot_2.clone());
        events.push(event_slot_2_2.clone());
        // Randomize the events so we insert in random orders in the DB
        events.shuffle(&mut thread_rng());
        cache.insert_multi_it(events.into_iter());

        let filter_1 = EventFilter {
            is_error: Some(true),
            ..Default::default()
        };

        let (_, filtered_events_1) = cache.get_filtered_sc_output_events(&filter_1);
        assert_eq!(filtered_events_1.len(), 1);
        assert!(filtered_events_1[0].context.is_error);
        assert_eq!(filtered_events_1[0].context.slot, slot_2);
        assert_eq!(filtered_events_1[0].context.index_in_slot, index_2_2);
    }

    #[test]
    #[serial]
    fn test_filter_optimisations() {
        // Test we iterate over the right number of rows when filtering

        let mut cache = setup();
        cache.max_entry_count = 10;

        let slot_1 = Slot::new(1, 0);
        let index_1_0 = 0;

        let dummy_addr =
            Address::from_str("AU12qePoXhNbYWE1jZuafqJong7bbq1jw3k89RgbMawbrdZpaasoA").unwrap();
        let emit_addr_1 =
            Address::from_str("AU122Em8qkqegdLb1eyH8rdkSCNEf7RZLeTJve4Q2inRPGiTJ2xNv").unwrap();
        let emit_addr_2 =
            Address::from_str("AU12WuVR1Td74q9eAbtYZUnk5jnRbUuUacyhQFwm217bV5v1mNqTZ").unwrap();

        let event = SCOutputEvent {
            context: EventExecutionContext {
                slot: slot_1,
                block: None,
                read_only: false,
                index_in_slot: index_1_0,
                call_stack: Default::default(),
                origin_operation_id: None,
                is_final: true,
                is_error: true,
            },
            data: "error foo bar".to_string(),
        };

        let to_insert_count = cache.max_entry_count - 5;
        let threshold = to_insert_count / 2;
        let mut events = (0..cache.max_entry_count - 5)
            .map(|i| {
                let mut event = event.clone();
                event.context.index_in_slot = i as u64;
                if i < threshold {
                    event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_1]);
                } else {
                    event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
                }
                event
            })
            .collect::<Vec<SCOutputEvent>>();

        let slot_2 = Slot::new(2, 0);
        let index_2_1 = 0u64;
        let event_slot_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_1;
            event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
            event
        };
        let index_2_2 = 256u64;
        let event_slot_2_2 = {
            let mut event = event.clone();
            event.context.slot = slot_2;
            event.context.index_in_slot = index_2_2;
            event.context.call_stack = VecDeque::from(vec![dummy_addr, emit_addr_2]);
            // event.context.is_error = true;
            event
        };
        events.push(event_slot_2.clone());
        events.push(event_slot_2_2.clone());
        // Randomize the events so we insert in random orders in the DB
        events.shuffle(&mut thread_rng());
        cache.insert_multi_it(events.into_iter());

        // Check if we correctly count the number of events in the DB with emit_addr_1 & emit_addr_2
        let emit_addr_1_count = cache
            .filter_item_estimate_count(
                &KeyIndent::EmitterAddress,
                &FilterItem::EmitterAddress(emit_addr_1),
            )
            .unwrap();
        let emit_addr_2_count = cache
            .filter_item_estimate_count(
                &KeyIndent::EmitterAddress,
                &FilterItem::EmitterAddress(emit_addr_2),
            )
            .unwrap();

        assert_eq!(emit_addr_1_count, (threshold) as u64);
        assert_eq!(emit_addr_2_count, (threshold + 1 + 2) as u64);

        // Check if we query first by emitter address then is_error

        let filter_1 = EventFilter {
            emitter_address: Some(emit_addr_1),
            is_error: Some(true),
            ..Default::default()
        };

        let (query_counts, _filtered_events_1) = cache.get_filtered_sc_output_events(&filter_1);
        println!("threshold: {:?}", threshold);
        println!("query_counts: {:?}", query_counts);

        // Check that we iter no more than needed (here: only 2 (== threshold) event with emit addr 1)
        assert_eq!(query_counts[0], threshold as u64);
        // For second filter (is_error) we could have iter more (all events have is_error = true)
        // but as soon as we found 2 items we could return (as the previous filter already limit the final count)
        assert_eq!(query_counts[1], threshold as u64);
    }
}
