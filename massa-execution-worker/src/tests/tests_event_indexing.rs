// Copyright (c) 2026 MASSA LABS <info@massa.net>

use super::universe::ExecutionForeignControllers;
use crate::context::ExecutionContext;
use massa_async_pool::{AsyncPool, AsyncPoolConfig};
use massa_execution_exports::{ExecutionConfig, ExecutionError};
use massa_hash::Hash;
use massa_models::{config::MIP_STORE_STATS_BLOCK_CONSIDERED, slot::Slot};
use massa_module_cache::{config::ModuleCacheConfig, controller::ModuleCache};
use massa_versioning::{
    mips::get_mip_list,
    versioning::{MipStatsConfig, MipStore},
};
use num::rational::Ratio;
use parking_lot::RwLock;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn event_index_uniqueness_repeated_rollback() {
    let controllers = ExecutionForeignControllers::new_with_mocks();
    controllers
        .final_state
        .write()
        .expect_get_async_pool()
        .return_const(AsyncPool::new(
            AsyncPoolConfig::default(),
            controllers.db.clone(),
        ));

    let config = ExecutionConfig::default();
    let cache_dir = TempDir::new().unwrap();
    let module_cache = Arc::new(RwLock::new(ModuleCache::new(ModuleCacheConfig {
        hd_cache_path: cache_dir.path().to_path_buf(),
        gas_costs: config.gas_costs.clone(),
        lru_cache_size: config.lru_cache_size,
        hd_cache_size: config.hd_cache_size,
        snip_amount: config.snip_amount,
        max_module_length: config.max_bytecode_size,
        condom_limits: config.condom_limits.clone(),
    })));
    let mip_store = MipStore::try_from((
        get_mip_list(),
        MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        },
    ))
    .unwrap();
    let mut context = ExecutionContext::new(
        config,
        controllers.final_state.clone(),
        Default::default(),
        module_cache,
        mip_store,
        Hash::compute_from(b"Genesis"),
    );
    let slot = Slot::new(1, 0);
    context.slot = slot;

    let emit = |context: &mut ExecutionContext, data: &str| {
        let event = context.event_create(data.to_owned(), false);
        context.event_emit(event);
    };
    emit(&mut context, "before rollback");
    let first_snapshot = context.get_snapshot();
    emit(&mut context, "first failed event");
    emit(&mut context, "second failed event");
    context.reset_to_snapshot(
        first_snapshot,
        ExecutionError::RuntimeError("first rollback".to_owned()),
    );
    emit(&mut context, "after first rollback");

    // A second failure must not reuse indexes or flag the intervening success.
    let second_snapshot = context.get_snapshot();
    emit(&mut context, "failed again");
    context.reset_to_snapshot(
        second_snapshot,
        ExecutionError::RuntimeError("second rollback".to_owned()),
    );
    emit(&mut context, "after second rollback");

    let events = &context.events.0;
    assert_eq!(events.len(), 8, "rollback must retain all emitted events");
    assert_eq!(
        events
            .iter()
            .map(|event| event.context.index_in_slot)
            .collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>(),
        "retained events and later emissions must have distinct increasing indexes"
    );
    assert!(events.iter().all(|event| event.context.slot == slot));
    assert_eq!(
        events
            .iter()
            .map(|event| event.context.is_error)
            .collect::<Vec<_>>(),
        vec![false, true, true, true, false, true, true, false]
    );
    for (index, data) in [
        (0, "before rollback"),
        (1, "first failed event"),
        (2, "second failed event"),
        (4, "after first rollback"),
        (5, "failed again"),
        (7, "after second rollback"),
    ] {
        assert_eq!(events[index].data, data);
    }
    for (index, message) in [(3, "first rollback"), (6, "second rollback")] {
        let error: serde_json::Value = serde_json::from_str(&events[index].data).unwrap();
        assert!(error["massa_execution_error"]
            .as_str()
            .unwrap()
            .contains(message));
    }
}
