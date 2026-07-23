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
//! WMAS is the same contract deployed at a different address on each network,
//! so a single embedded bytecode serves all of them; only the target address is
//! selected per network via `chain_id` (see [`wmas_address`]).
//!
//! Activation: this patch rides on the release's `MIP-0002-BugFix`, which already
//! bumps `MipComponent::Execution` to `2` (= [`WMAS_PATCH_EXEC_VERSION`]) in
//! `massa-versioning/src/mips.rs`. It therefore applies at that MIP's activation
//! slot, together with the rest of the next release's breaking changes — there
//! is no separate WMAS MIP.
//!
//! IMPORTANT: because it is bundled with `MIP-0002-BugFix` (which has real
//! activation dates), this patch is NOT inert until a future date. Before that
//! release ships, `wmas_bytecode.rs` MUST be regenerated from the reproducible,
//! audited build of the deployed WMAS contract (it currently embeds a dev build)
//! and the per-network `WMAS_ADDRESS_*` constants verified. The fail-safe in
//! `execute_slot` only skips on an unknown chain_id or missing contract, so
//! a wrong-but-valid blob would still be applied.

use massa_models::{address::Address, bytecode::Bytecode};
use std::str::FromStr;

/// Execution component version at which the WMAS bytecode patch activates.
pub const WMAS_PATCH_EXEC_VERSION: u32 = 2;

/// Chain ids of the networks where WMAS is deployed (see
/// `massa_models::config::CHAINID`).
const MAINNET_CHAIN_ID: u64 = 77658377;
const BUILDNET_CHAIN_ID: u64 = 77658366;

/// Deployed WMAS address on mainnet.
pub const WMAS_ADDRESS_MAINNET: &str = "AS12U4TZfNK7qoLyEERBBRDMu8nm5MKoRzPXDXans4v9wdATZedz9";
/// Deployed WMAS address on buildnet.
pub const WMAS_ADDRESS_BUILDNET: &str = "AS12FW5Rs5YN2zdpEnqwj4iHUUPt9R4Eqjq2qtpJFNKW3mn33RuLU";

/// Audited fixed WMAS bytecode, embedded as an in-source byte array (see
/// [`crate::wmas_bytecode`]).
///
/// TODO(release): regenerate `wmas_bytecode.rs` from the reproducible, audited
/// build of the actually deployed WMAS contract before enabling the activation
/// MIP.
pub const PATCHED_WMAS_BYTECODE: &[u8] = crate::wmas_bytecode::PATCHED_WMAS_BYTECODE;

// Some checks at compile time that should not be ignored!
#[allow(clippy::const_is_empty)]
const _: () = {
    assert!(!PATCHED_WMAS_BYTECODE.is_empty());
};

/// Returns the WMAS address to patch for the network identified by `chain_id`,
/// or `None` if the network has no known WMAS deployment (e.g. sandbox/labnet)
/// or the address constant is malformed. Returning `None` rather than panicking
/// keeps a misconfiguration from aborting consensus execution.
pub fn wmas_address(chain_id: u64) -> Option<Address> {
    let addr = match chain_id {
        MAINNET_CHAIN_ID => WMAS_ADDRESS_MAINNET,
        BUILDNET_CHAIN_ID => WMAS_ADDRESS_BUILDNET,
        _ => return None,
    };
    Address::from_str(addr).ok()
}

/// Returns the patched WMAS bytecode.
pub fn patched_wmas_bytecode() -> Bytecode {
    Bytecode(PATCHED_WMAS_BYTECODE.to_vec())
}
