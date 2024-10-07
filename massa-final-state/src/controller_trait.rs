use massa_async_pool::AsyncPool;
use massa_db_exports::{DBBatch, ShareableMassaDBController};
use massa_deferred_calls::DeferredCallRegistry;
use massa_executed_ops::ExecutedDenunciations;
use massa_hash::Hash;
use massa_ledger_exports::LedgerController;
use massa_models::{operation::OperationId, slot::Slot};
use massa_pos_exports::PoSFinalState;
use massa_versioning::versioning::MipStore;

use crate::{FinalStateError, StateChanges};

/// Trait for final state controller.
#[cfg_attr(feature = "test-exports", mockall::automock)]
pub trait FinalStateController: Send + Sync {
    /// Get the fingerprint (hash) of the final state.
    /// Note that only one atomic write per final slot occurs, so this can be safely queried at any time.
    fn get_fingerprint(&self) -> Hash;

    /// Get the slot at the end of which the final state is attached
    fn get_slot(&self) -> Slot;

    /// Gets the hash of the execution trail
    fn get_execution_trail_hash(&self) -> Hash;

    /// Reset the final state to the initial state.
    ///
    /// USED ONLY FOR BOOTSTRAP
    fn reset(&mut self);

    /// Performs the initial draws.
    fn compute_initial_draws(&mut self) -> Result<(), FinalStateError>;

    /// Applies changes to the execution state at a given slot, and settles that slot forever.
    /// Once this is called, the state is attached at the output of the provided slot.
    ///
    /// Panics if the new slot is not the one coming just after the current one.
    fn finalize(&mut self, slot: Slot, changes: StateChanges);

    /// After bootstrap or load from disk, recompute all the caches.
    fn recompute_caches(&mut self);

    /// Deserialize the entire DB and check the data. Useful to check after bootstrap.
    fn is_db_valid(&self) -> bool;

    /// Initialize the execution trail hash to zero.
    fn init_execution_trail_hash_to_batch(&mut self, batch: &mut DBBatch);

    /// Get ledger
    #[allow(clippy::borrowed_box)]
    fn get_ledger(&self) -> &Box<dyn LedgerController>;

    /// Get ledger mut
    fn get_ledger_mut(&mut self) -> &mut Box<dyn LedgerController>;

    /// Get async pool
    fn get_async_pool(&self) -> &AsyncPool;

    /// Get pos state
    fn get_pos_state(&self) -> &PoSFinalState;

    /// Get pos state mut
    fn get_pos_state_mut(&mut self) -> &mut PoSFinalState;

    /// check if an operation is in the executed ops
    fn executed_ops_contains(&self, op_id: &OperationId) -> bool;

    /// Get the executed status ops
    fn get_ops_exec_status(&self, batch: &[OperationId]) -> Vec<Option<bool>>;

    /// Get executed denunciations
    fn get_executed_denunciations(&self) -> &ExecutedDenunciations;

    /// Get the database
    fn get_database(&self) -> &ShareableMassaDBController;

    /// Get last start period
    fn get_last_start_period(&self) -> u64;

    /// Set last start period
    fn set_last_start_period(&mut self, last_start_period: u64);

    /// Get last slot before downtime
    fn get_last_slot_before_downtime(&self) -> &Option<Slot>;

    /// Set last slot before downtime
    fn set_last_slot_before_downtime(&mut self, last_slot_before_downtime: Option<Slot>);

    /// Get MIP Store
    fn get_mip_store(&self) -> &MipStore;

    /// Get mutable reference to MIP Store
    fn get_mip_store_mut(&mut self) -> &mut MipStore;

    /// Get deferred call registry
    fn get_deferred_call_registry(&self) -> &DeferredCallRegistry;
}
