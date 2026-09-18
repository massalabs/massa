//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to the final state

use massa_async_pool::AsyncPoolChanges;
use massa_deferred_calls::registry_changes::DeferredCallRegistryChanges;
use massa_executed_ops::{ExecutedDenunciationsChanges, ExecutedOpsChanges};
use massa_ledger_exports::LedgerChanges;
use massa_models::types::SetOrKeep;
use massa_pos_exports::PoSChanges;
use serde::{Deserialize, Serialize};

/// represents changes that can be applied to the execution state
#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct StateChanges {
    /// ledger changes
    pub ledger_changes: LedgerChanges,
    /// asynchronous pool changes
    pub async_pool_changes: AsyncPoolChanges,
    /// deferred call changes
    pub deferred_call_changes: DeferredCallRegistryChanges,
    /// roll state changes
    pub pos_changes: PoSChanges,
    /// executed operations changes
    pub executed_ops_changes: ExecutedOpsChanges,
    /// executed denunciations changes
    pub executed_denunciations_changes: ExecutedDenunciationsChanges,
    /// execution trail hash change
    pub execution_trail_hash_change: SetOrKeep<massa_hash::Hash>,
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use massa_ledger_exports::LedgerEntryUpdate;
    use massa_models::{
        address::Address, amount::Amount, async_msg::AsyncMessage, bytecode::Bytecode, slot::Slot,
        types::SetUpdateOrDelete,
    };

    use super::*;

    impl PartialEq<StateChanges> for StateChanges {
        fn eq(&self, other: &StateChanges) -> bool {
            self.ledger_changes == other.ledger_changes &&
                self.async_pool_changes == other.async_pool_changes &&
                // pos_changes
                self.pos_changes.seed_bits == other.pos_changes.seed_bits &&
                self.pos_changes.roll_changes == other.pos_changes.roll_changes &&
                self.pos_changes.production_stats == other.pos_changes.production_stats &&
                self.pos_changes.deferred_credits.credits == other.pos_changes.deferred_credits.credits &&
                self.executed_ops_changes == other.executed_ops_changes &&
                self.executed_denunciations_changes == other.executed_denunciations_changes &&
                self.execution_trail_hash_change == other.execution_trail_hash_change
        }
    }

    #[test]
    fn test_state_changes_serde() {
        let mut state_changes = StateChanges::default();
        let message = AsyncMessage::new(
            Slot::new(1, 0),
            0,
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            String::from("test"),
            10000000,
            Amount::from_str("1").unwrap(),
            Amount::from_str("1").unwrap(),
            Slot::new(2, 0),
            Slot::new(3, 0),
            vec![1, 2, 3, 4],
            None,
            None,
        );
        let mut async_pool_changes = AsyncPoolChanges::default();
        async_pool_changes
            .0
            .insert(message.compute_id(), SetUpdateOrDelete::Set(message));
        state_changes.async_pool_changes = async_pool_changes;

        let amount = Amount::from_str("1").unwrap();
        let bytecode = Bytecode(vec![1, 2, 3]);
        let mut datastore = BTreeMap::new();
        datastore.insert(
            b"hello".to_vec(),
            massa_models::types::SetOrDelete::Set(b"world".to_vec()),
        );
        let ledger_entry = LedgerEntryUpdate {
            balance: SetOrKeep::Set(amount),
            bytecode: SetOrKeep::Set(bytecode),
            datastore,
        };
        let mut ledger_changes = LedgerChanges::default();
        ledger_changes.0.insert(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            SetUpdateOrDelete::Update(ledger_entry),
        );
        state_changes.ledger_changes = ledger_changes;
        let serialized = serde_json::to_string(&state_changes).unwrap();
        let state_changes_deser = serde_json::from_str(&serialized).unwrap();
        assert_eq!(state_changes, state_changes_deser);
    }
}
