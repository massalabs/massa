//! One-time, versioning-gated patch that replaces the bytecode of the deployed
//! WMAS contract to fix the storage-cost drain (a `transfer`/`increaseAllowance`
//! to a fresh address makes WMAS pay, from its own locked MAS, for the new
//! datastore balance entry).
//!
//! WMAS is a widely-used singleton that cannot be redeployed without forcing
//! every holder and integration to migrate. Instead, at the activation of
//! [`MipComponent::Execution`] version [`WMAS_PATCH_EXEC_VERSION`], every node
//! deterministically overwrites the WMAS bytecode with an audited fixed build.
//!
//! Safety of the approach:
//! - Bytecode is stored separately from the datastore and the coin balance, so
//!   all WMAS balances and all locked MAS are preserved by the swap.
//! - The module cache is keyed by bytecode hash (`Hash::compute_from`), so no
//!   explicit invalidation is required: the new bytecode hashes differently and
//!   is recompiled on first use.
//! - The change is applied inside `execute_slot`, which is shared by both
//!   candidate and final execution, so speculative and final states agree. The
//!   resulting bytecode change is part of the slot's ledger changes and thus of
//!   the final state, so nodes bootstrapping after activation receive the
//!   already-patched bytecode.
//!
//! Activation (done at release time, NOT in this commit):
//! register a MIP that bumps `MipComponent::Execution` to
//! `WMAS_PATCH_EXEC_VERSION` in `massa-versioning/src/mips.rs`, e.g.:
//! ```ignore
//! MipInfo {
//!     name: "MIP-XXXX-WMAS-BytecodePatch".to_string(),
//!     version: <next>,
//!     components: BTreeMap::from([(MipComponent::Execution, WMAS_PATCH_EXEC_VERSION)]),
//!     start: /* chosen date */, timeout: /* + window */, activation_delay: /* delay */,
//! }
//! ```
//! Until such a MIP exists, `execution_component_version` never reaches
//! `WMAS_PATCH_EXEC_VERSION` and this code is inert.

use massa_models::{
    address::Address, bytecode::Bytecode, slot::Slot, timeslots::get_block_slot_timestamp,
};
use massa_time::MassaTime;
use massa_versioning::versioning::{MipComponent, MipStore};
use std::str::FromStr;

/// Execution component version at which the WMAS bytecode patch activates.
pub const WMAS_PATCH_EXEC_VERSION: u32 = 2;

/// Address of the deployed WMAS contract to patch.
///
/// TODO(release): set to the verified WMAS mainnet address before enabling the
/// activation MIP. Patching the wrong address would be catastrophic.
pub const WMAS_ADDRESS: &str = "AS_REPLACE_WITH_VERIFIED_WMAS_ADDRESS";

/// Audited, reproducibly-built fixed WMAS bytecode, embedded at build time.
///
/// TODO(release): replace `resources/wmas_patched.wasm` with the reproducible
/// build of the fixed WMAS contract (massa-standards) before enabling the
/// activation MIP. It is left empty here so the crate builds while the code is
/// inert (gated behind a MIP that does not exist yet); [`patched_wmas_bytecode`]
/// refuses to apply an empty bytecode as a fail-safe.
pub const PATCHED_WMAS_BYTECODE: &[u8] = include_bytes!("../resources/wmas_patched.wasm");

/// Returns the WMAS address to patch.
pub fn wmas_address() -> Address {
    Address::from_str(WMAS_ADDRESS).expect("invalid WMAS_ADDRESS constant")
}

/// Returns the patched WMAS bytecode, or `None` if it has not been embedded yet
/// (fail-safe: never apply an empty bytecode).
pub fn patched_wmas_bytecode() -> Option<Bytecode> {
    if PATCHED_WMAS_BYTECODE.is_empty() {
        return None;
    }
    Some(Bytecode(PATCHED_WMAS_BYTECODE.to_vec()))
}

/// Execution component version active at `slot`.
fn execution_version_at_slot(
    mip_store: &MipStore,
    slot: &Slot,
    thread_count: u8,
    t0: MassaTime,
    genesis_timestamp: MassaTime,
) -> u32 {
    let ts = get_block_slot_timestamp(thread_count, t0, genesis_timestamp, *slot)
        .unwrap_or(genesis_timestamp);
    mip_store.get_latest_component_version_at(&MipComponent::Execution, ts)
}

/// Returns `true` iff `slot` is the exact slot at which the WMAS patch must be
/// applied: the first slot whose execution component version reaches
/// [`WMAS_PATCH_EXEC_VERSION`]. Deterministic across all nodes, so it fires on
/// exactly one slot network-wide.
pub fn is_wmas_patch_activation_slot(
    mip_store: &MipStore,
    slot: &Slot,
    thread_count: u8,
    t0: MassaTime,
    genesis_timestamp: MassaTime,
) -> bool {
    let current = execution_version_at_slot(mip_store, slot, thread_count, t0, genesis_timestamp);
    if current < WMAS_PATCH_EXEC_VERSION {
        return false;
    }
    // Only apply on the transition: the previous slot must be below the target.
    match slot.get_prev_slot(thread_count) {
        Ok(prev) => {
            execution_version_at_slot(mip_store, &prev, thread_count, t0, genesis_timestamp)
                < WMAS_PATCH_EXEC_VERSION
        }
        // No previous slot (genesis): treat as a transition.
        Err(_) => true,
    }
}
