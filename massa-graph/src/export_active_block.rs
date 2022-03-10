use crate::error::{GraphError, GraphResult as Result};
use massa_models::{
    active_block::ActiveBlock,
    constants::*,
    ledger_models::{LedgerChange, LedgerChanges},
    prehash::{BuildMap, Map, Set},
    rolls::{RollUpdate, RollUpdates},
    signed::Signable,
    *,
};
use serde::{Deserialize, Serialize};

/// Exportable version of ActiveBlock
/// Fields that can be easily recomputed were left out
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportActiveBlock {
    /// The block itself, as it was created
    pub block: Block,
    /// one (block id, period) per thread ( if not genesis )
    pub parents: Vec<(BlockId, u64)>,
    /// one HashMap<Block id, period> per thread (blocks that need to be kept)
    /// Children reference that block as a parent
    pub children: Vec<Map<BlockId, u64>>,
    /// dependencies required for validity check
    pub dependencies: Set<BlockId>,
    /// ie has its fitness reached the given threshold
    pub is_final: bool,
    /// Changes caused by this block
    pub block_ledger_changes: LedgerChanges,
    /// Address -> RollUpdate
    pub roll_updates: RollUpdates,
    /// list of (period, address, did_create) for all block/endorsement creation events
    pub production_events: Vec<(u64, Address, bool)>,
}

impl From<&ActiveBlock> for ExportActiveBlock {
    fn from(a_block: &ActiveBlock) -> Self {
        ExportActiveBlock {
            block: a_block.block.clone(),
            parents: a_block.parents.clone(),
            children: a_block.children.clone(),
            dependencies: a_block.dependencies.clone(),
            is_final: a_block.is_final,
            block_ledger_changes: a_block.block_ledger_changes.clone(),
            roll_updates: a_block.roll_updates.clone(),
            production_events: a_block.production_events.clone(),
        }
    }
}

impl TryFrom<ExportActiveBlock> for ActiveBlock {
    fn try_from(a_block: ExportActiveBlock) -> Result<ActiveBlock> {
        let operation_set = a_block
            .block
            .operations
            .iter()
            .enumerate()
            .map(|(idx, op)| match op.content.compute_id() {
                Ok(id) => Ok((id, (idx, op.content.expire_period))),
                Err(e) => Err(e),
            })
            .collect::<Result<_, _>>()?;

        let endorsement_ids = a_block
            .block
            .header
            .content
            .endorsements
            .iter()
            .map(|endo| Ok((endo.content.compute_id()?, endo.content.index)))
            .collect::<Result<_>>()?;

        let addresses_to_operations = a_block.block.involved_addresses(&operation_set)?;
        let addresses_to_endorsements =
            a_block.block.addresses_to_endorsements(&endorsement_ids)?;
        Ok(ActiveBlock {
            creator_address: Address::from_public_key(&a_block.block.header.content.creator),
            block: a_block.block,
            parents: a_block.parents,
            children: a_block.children,
            dependencies: a_block.dependencies,
            descendants: Default::default(), // will be computed once the full graph is available
            is_final: a_block.is_final,
            block_ledger_changes: a_block.block_ledger_changes,
            operation_set,
            endorsement_ids,
            addresses_to_operations,
            roll_updates: a_block.roll_updates,
            production_events: a_block.production_events,
            addresses_to_endorsements,
        })
    }

    type Error = GraphError;
}

impl SerializeCompact for ExportActiveBlock {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // is_final
        if self.is_final {
            res.push(1);
        } else {
            res.push(0);
        }

        // block
        res.extend(self.block.to_bytes_compact()?);

        // parents (note: there should be none if slot period=0)
        if self.parents.is_empty() {
            res.push(0);
        } else {
            res.push(1);
        }
        for (hash, period) in self.parents.iter() {
            res.extend(&hash.to_bytes());
            res.extend(period.to_varint_bytes());
        }

        // children
        let children_count: u32 = self.children.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many children in ActiveBlock: {}", err))
        })?;
        res.extend(children_count.to_varint_bytes());
        for map in self.children.iter() {
            let map_count: u32 = map.len().try_into().map_err(|err| {
                ModelsError::SerializeError(format!(
                    "too many entry in children map in ActiveBlock: {}",
                    err
                ))
            })?;
            res.extend(map_count.to_varint_bytes());
            for (hash, period) in map {
                res.extend(&hash.to_bytes());
                res.extend(period.to_varint_bytes());
            }
        }

        // dependencies
        let dependencies_count: u32 = self.dependencies.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many dependencies in ActiveBlock: {}", err))
        })?;
        res.extend(dependencies_count.to_varint_bytes());
        for dep in self.dependencies.iter() {
            res.extend(&dep.to_bytes());
        }

        // block_ledger_change
        let block_ledger_change_count: u32 =
            self.block_ledger_changes
                .0
                .len()
                .try_into()
                .map_err(|err| {
                    ModelsError::SerializeError(format!(
                        "too many block_ledger_change in ActiveBlock: {}",
                        err
                    ))
                })?;
        res.extend(block_ledger_change_count.to_varint_bytes());
        for (addr, change) in self.block_ledger_changes.0.iter() {
            res.extend(&addr.to_bytes());
            res.extend(change.to_bytes_compact()?);
        }

        // roll updates
        let roll_updates_count: u32 = self.roll_updates.0.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many roll updates in ActiveBlock: {}", err))
        })?;
        res.extend(roll_updates_count.to_varint_bytes());
        for (addr, roll_update) in self.roll_updates.0.iter() {
            res.extend(addr.to_bytes());
            res.extend(roll_update.to_bytes_compact()?);
        }

        // creation events
        let production_events_count: u32 =
            self.production_events.len().try_into().map_err(|err| {
                ModelsError::SerializeError(format!(
                    "too many creation events in ActiveBlock: {}",
                    err
                ))
            })?;
        res.extend(production_events_count.to_varint_bytes());
        for (period, addr, has_created) in self.production_events.iter() {
            res.extend(period.to_varint_bytes());
            res.extend(addr.to_bytes());
            res.push(if *has_created { 1u8 } else { 0u8 });
        }

        Ok(res)
    }
}

impl DeserializeCompact for ExportActiveBlock {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;
        let (parent_count, max_bootstrap_children, max_bootstrap_deps, max_bootstrap_pos_entries) =
            with_serialization_context(|context| {
                (
                    context.thread_count,
                    context.max_bootstrap_children,
                    context.max_bootstrap_deps,
                    context.max_bootstrap_pos_entries,
                )
            });

        // is_final
        let is_final_u8 = u8_from_slice(buffer)?;
        cursor += 1;
        let is_final = is_final_u8 != 0;

        // block
        let (block, delta) = Block::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // parents
        let has_parents = u8_from_slice(&buffer[cursor..])?;
        cursor += 1;
        let parents = if has_parents == 1 {
            let mut parents: Vec<(BlockId, u64)> = Vec::with_capacity(parent_count as usize);
            for _ in 0..parent_count {
                let parent_h = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += BLOCK_ID_SIZE_BYTES;
                let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                parents.push((parent_h, period));
            }
            parents
        } else if has_parents == 0 {
            Vec::new()
        } else {
            return Err(ModelsError::SerializeError(
                "ActiveBlock from_bytes_compact bad has parents flags.".into(),
            ));
        };

        // children
        let (children_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        let parent_count_u32: u32 = parent_count.into();
        if children_count > parent_count_u32 {
            return Err(ModelsError::DeserializeError(
                "too many threads with children to deserialize".to_string(),
            ));
        }
        cursor += delta;
        let mut children: Vec<Map<BlockId, u64>> = Vec::with_capacity(children_count as usize);
        for _ in 0..(children_count as usize) {
            let (map_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            if map_count > max_bootstrap_children {
                return Err(ModelsError::DeserializeError(
                    "too many children to deserialize".to_string(),
                ));
            }
            cursor += delta;
            let mut map: Map<BlockId, u64> =
                Map::with_capacity_and_hasher(map_count as usize, BuildMap::default());
            for _ in 0..(map_count as usize) {
                let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += BLOCK_ID_SIZE_BYTES;
                let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;
                map.insert(hash, period);
            }
            children.push(map);
        }

        // dependencies
        let (dependencies_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if dependencies_count > max_bootstrap_deps {
            return Err(ModelsError::DeserializeError(
                "too many dependencies to deserialize".to_string(),
            ));
        }
        cursor += delta;
        let mut dependencies = Set::<BlockId>::with_capacity_and_hasher(
            dependencies_count as usize,
            BuildMap::default(),
        );
        for _ in 0..(dependencies_count as usize) {
            let dep = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += BLOCK_ID_SIZE_BYTES;
            dependencies.insert(dep);
        }

        // block_ledger_changes
        let (block_ledger_change_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        // TODO: count check ... see #1200
        cursor += delta;
        let mut block_ledger_changes = LedgerChanges(Map::with_capacity_and_hasher(
            block_ledger_change_count as usize,
            BuildMap::default(),
        ));
        for _ in 0..block_ledger_change_count {
            let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;
            let (change, delta) = LedgerChange::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            block_ledger_changes.0.insert(address, change);
        }

        // roll_updates
        let (roll_updates_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if roll_updates_count > max_bootstrap_pos_entries {
            return Err(ModelsError::DeserializeError(
                "too many roll updates to deserialize".to_string(),
            ));
        }
        cursor += delta;
        let mut roll_updates = RollUpdates(Map::with_capacity_and_hasher(
            roll_updates_count as usize,
            BuildMap::default(),
        ));
        for _ in 0..roll_updates_count {
            let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;
            let (roll_update, delta) = RollUpdate::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            roll_updates.0.insert(address, roll_update);
        }

        // production_events
        let (production_events_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        // TODO: count check ... see #1200
        cursor += delta;
        let mut production_events: Vec<(u64, Address, bool)> =
            Vec::with_capacity(production_events_count as usize);
        for _ in 0..production_events_count {
            let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;
            let has_created = match u8_from_slice(&buffer[cursor..])? {
                0u8 => false,
                1u8 => true,
                _ => {
                    return Err(ModelsError::SerializeError(
                        "could not deserialize active_block.production_events.has_created".into(),
                    ))
                }
            };
            cursor += 1;
            production_events.push((period, address, has_created));
        }

        Ok((
            ExportActiveBlock {
                is_final,
                block,
                parents,
                children,
                dependencies,
                block_ledger_changes,
                roll_updates,
                production_events,
            },
            cursor,
        ))
    }
}
