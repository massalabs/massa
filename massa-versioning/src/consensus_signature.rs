//! Consensus signature layout gating (F90 / PDF #11).
//!
//! Endorsements and block headers used to be signed with a chain-agnostic
//! hash: the signed hash did not include the chain id, so two legitimate
//! same-(slot, index) signatures produced by the same validator on two
//! different chains (e.g. MAINNET + LABNET) could be combined into a
//! denunciation on either chain and slash the validator for entirely normal
//! cross-chain operation.
//!
//! The fix folds the chain id into the signed hash, deployed through the
//! existing MIP-0002 network-versioning mechanism (the release that bumps
//! `MipComponent::Execution` to `EXEC_SIGNATURES_CHAIN_ID_VERSION`), so
//! upgraded and un-upgraded nodes keep agreeing on signature validity
//! across the activation boundary - no flag-day change.
//!
//! This module exposes the single helper that decides, for a given slot,
//! which signed-hash layout to use:
//! * `None`  -> legacy (chain-agnostic) layout, identical to the pre-fix one.
//! * `Some(chain_id)` -> chain-scoped layout, folding `chain_id` into the
//!   signed hash.
//!
//! The decision is based on the network-agreed active `Execution` component
//! version at the message's slot timestamp - not any self-claimed header
//! field. Signing, on-receive verification, denunciation creation and
//! denunciation execution must all call this with the same
//! `(mip_store, chain_id, slot_ts)` so they flip layouts together at the
//! deterministic activation slot.

use massa_time::MassaTime;

use crate::versioning::{MipComponent, MipStore};

/// `Execution` component version at which the chain-scoped consensus
/// signature layout activates (i.e. `chain_id` is folded into the signed
/// hash of endorsements and block headers). Must match MIP-0002's
/// `Execution` component version
/// (`massa_execution_worker::wmas_patch::WMAS_PATCH_EXEC_VERSION`).
pub const EXEC_SIGNATURES_CHAIN_ID_VERSION: u32 = 2;

/// Returns the `sig_chain_id` to use for signing / verifying an endorsement
/// or block header whose slot timestamp is `slot_ts` on the local chain
/// identified by `chain_id`.
///
/// Returns `Some(chain_id)` once the `Execution` component is active at
/// version >= [`EXEC_SIGNATURES_CHAIN_ID_VERSION`] for that slot, `None`
/// otherwise (legacy layout).
///
/// Every signing / verification / denunciation-reconstruction site must
/// call this with the message's own slot timestamp so all nodes switch
/// layouts simultaneously at the activation slot (no partition).
pub fn sig_chain_id_for_slot(
    mip_store: &MipStore,
    chain_id: u64,
    slot_ts: MassaTime,
) -> Option<u64> {
    if mip_store.get_latest_component_version_at(&MipComponent::Execution, slot_ts)
        >= EXEC_SIGNATURES_CHAIN_ID_VERSION
    {
        Some(chain_id)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use massa_hash::Hash;
    use massa_models::block_header::{BlockHeader, BlockHeaderSerializer};
    use massa_models::block_id::BlockId;
    use massa_models::config::{CHAINID, MIP_STORE_STATS_BLOCK_CONSIDERED};
    use massa_models::denunciation::Denunciation;
    use massa_models::endorsement::{Endorsement, EndorsementSerializer};
    use massa_models::secure_share::{Id, SecureShareContent};
    use massa_models::slot::Slot;
    use massa_signature::KeyPair;
    use num::rational::Ratio;

    use crate::test_helpers::versioning_helpers::advance_state_until;
    use crate::versioning::{ComponentState, MipComponent, MipInfo, MipStatsConfig, MipStore};

    /// MIP-0002-shaped store: Execution v2 is currently Active. Layout is still
    /// decided per timestamp via `state_at`, so slots before `start` stay legacy.
    fn store_with_execution_v2_active() -> (MipStore, MassaTime, MassaTime) {
        let start = MassaTime::from_millis(10_000);
        let timeout = MassaTime::from_millis(100_000);
        let activation_delay = MassaTime::from_millis(5_000);
        let mi = MipInfo {
            name: "MIP-0002-BugFix".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Execution, 2)]),
            start,
            timeout,
            activation_delay,
        };
        let ms = advance_state_until(ComponentState::active(MassaTime::from_millis(0)), &mi);
        let stats = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let store = MipStore::try_from(([(mi, ms)], stats)).expect("mip store");

        let before = start.saturating_sub(MassaTime::from_millis(1));
        let active_at = (0..400)
            .map(|i| start.saturating_add(MassaTime::from_millis(i * 100)))
            .find(|ts| {
                store.get_latest_component_version_at(&MipComponent::Execution, *ts)
                    >= EXEC_SIGNATURES_CHAIN_ID_VERSION
            })
            .expect("Execution v2 should become Active after start");
        (store, before, active_at)
    }

    fn make_endorsement(
        keypair: &KeyPair,
        slot: Slot,
        tag: &str,
        chain_id: u64,
        sig_chain_id: Option<u64>,
    ) -> massa_models::endorsement::SecureShareEndorsement {
        Endorsement::new_verifiable(
            Endorsement {
                slot,
                index: 0,
                endorsed_block: BlockId::new(Hash::compute_from(tag.as_bytes())),
            },
            EndorsementSerializer::new(),
            keypair,
            chain_id,
            sig_chain_id,
        )
        .unwrap()
    }

    fn make_header(
        keypair: &KeyPair,
        slot: Slot,
        tag: &str,
        chain_id: u64,
        sig_chain_id: Option<u64>,
    ) -> massa_models::block_header::SecuredHeader {
        BlockHeader::new_verifiable(
            BlockHeader {
                current_version: 0,
                announced_version: None,
                slot,
                parents: Vec::new(),
                operation_merkle_root: Hash::compute_from(tag.as_bytes()),
                endorsements: Vec::new(),
                denunciations: Vec::new(),
            },
            BlockHeaderSerializer::new(),
            keypair,
            chain_id,
            sig_chain_id,
        )
        .unwrap()
    }

    #[test]
    fn test_sig_chain_id_for_slot_flips_at_execution_v2_active() {
        let chain = *CHAINID;
        let (store, before, active_at) = store_with_execution_v2_active();

        assert_eq!(sig_chain_id_for_slot(&store, chain, before), None);
        assert_eq!(sig_chain_id_for_slot(&store, chain, active_at), Some(chain));
    }

    /// Full migration: the helper, signing, and denunciation reconstruction
    /// all use the same layout at a given slot timestamp, so nodes that query
    /// the same MipStore agree (no partition). Covers endorsements and headers.
    #[test]
    fn test_mip_store_migration_endorsement_and_header_no_partition() {
        let chain = *CHAINID;
        let other_chain = chain.wrapping_add(1);
        let (store, before, active_at) = store_with_execution_v2_active();
        let keypair = KeyPair::generate(0).unwrap();
        let slot = Slot::new(3, 7);

        // --- pre-activation: helper says None; legacy denunciations verify ---
        let pre = sig_chain_id_for_slot(&store, chain, before);
        assert_eq!(pre, None);

        let e1 = make_endorsement(&keypair, slot, "e_pre_1", chain, pre);
        let e2 = make_endorsement(&keypair, slot, "e_pre_2", chain, pre);
        let de_e_pre = Denunciation::try_from((&e1, &e2, pre)).unwrap();
        assert!(de_e_pre.is_valid_with_chain(pre));
        assert!(!de_e_pre.is_valid_with_chain(Some(chain)));

        let h1 = make_header(&keypair, slot, "h_pre_1", chain, pre);
        let h2 = make_header(&keypair, slot, "h_pre_2", chain, pre);
        let de_h_pre = Denunciation::try_from((&h1, &h2, pre)).unwrap();
        assert!(de_h_pre.is_valid_with_chain(pre));
        assert!(!de_h_pre.is_valid_with_chain(Some(chain)));

        // --- post-activation: helper says Some(chain); scoped denunciations verify ---
        let post = sig_chain_id_for_slot(&store, chain, active_at);
        assert_eq!(post, Some(chain));

        let e3 = make_endorsement(&keypair, slot, "e_post_1", chain, post);
        let e4 = make_endorsement(&keypair, slot, "e_post_2", chain, post);
        let de_e_post = Denunciation::try_from((&e3, &e4, post)).unwrap();
        assert!(de_e_post.is_valid_with_chain(post));
        assert!(!de_e_post.is_valid_with_chain(None));

        let h3 = make_header(&keypair, slot, "h_post_1", chain, post);
        let h4 = make_header(&keypair, slot, "h_post_2", chain, post);
        let de_h_post = Denunciation::try_from((&h3, &h4, post)).unwrap();
        assert!(de_h_post.is_valid_with_chain(post));
        assert!(!de_h_post.is_valid_with_chain(None));

        // Cross-chain combination fails once the helper is in the scoped layout.
        let e_other = make_endorsement(&keypair, slot, "e_other", other_chain, Some(other_chain));
        assert!(Denunciation::try_from((&e3, &e_other, post)).is_err());
        let h_other = make_header(&keypair, slot, "h_other", other_chain, Some(other_chain));
        assert!(Denunciation::try_from((&h3, &h_other, post)).is_err());
    }
}
