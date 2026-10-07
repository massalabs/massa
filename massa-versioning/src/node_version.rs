//! Node version compatibility checked during bootstrap and protocol handshakes.
//!
//! Peers must run the same instance and major version. Once `MIP-0002` is
//! active, peers older than the first release shipping it
//! ([`MIP_0002_NODE_VERSION`]) are rejected as well: they would not follow
//! the post-activation rules.

use massa_models::version::Version;
use massa_time::MassaTime;

use crate::mips::{MIP_0002_EXECUTION_VERSION, MIP_0002_NODE_VERSION};
use crate::versioning::{MipComponent, MipStore};

/// true if a peer running `peer_version` is compatible with us at `now`
///
/// A node never requires its peers to be newer than itself, so the `MIP-0002`
/// requirement only applies when `our_version` ships it.
pub fn is_peer_version_compatible(
    mip_store: &MipStore,
    our_version: &Version,
    peer_version: &Version,
    now: MassaTime,
) -> bool {
    if !our_version.is_compatible(peer_version) {
        return false;
    }

    let (major, minor) = MIP_0002_NODE_VERSION;
    let mip_0002_active = mip_store.get_latest_component_version_at(&MipComponent::Execution, now)
        >= MIP_0002_EXECUTION_VERSION;

    !mip_0002_active
        || !our_version.is_at_least(major, minor)
        || peer_version.is_at_least(major, minor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use massa_models::config::MIP_STORE_STATS_BLOCK_CONSIDERED;
    use num::rational::Ratio;

    use crate::test_helpers::versioning_helpers::advance_state_until;
    use crate::versioning::{ComponentState, MipInfo, MipStatsConfig};

    /// Store with `MIP-0002` active, plus a timestamp before and after activation.
    fn store_with_mip_0002_active() -> (MipStore, MassaTime, MassaTime) {
        let start = MassaTime::from_millis(10_000);
        let mi = MipInfo {
            name: "MIP-0002-BugFix".to_string(),
            version: 2,
            components: BTreeMap::from([(MipComponent::Execution, MIP_0002_EXECUTION_VERSION)]),
            start,
            timeout: MassaTime::from_millis(100_000),
            activation_delay: MassaTime::from_millis(5_000),
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
                    >= MIP_0002_EXECUTION_VERSION
            })
            .expect("MIP-0002 should become active after start");
        (store, before, active_at)
    }

    fn v(s: &str) -> Version {
        Version::from_str(s).unwrap()
    }

    fn mip_0002_release() -> Version {
        let (major, minor) = MIP_0002_NODE_VERSION;
        v(&format!("MAIN.{}.{}", major, minor))
    }

    fn pre_mip_0002_release() -> Version {
        let (major, minor) = MIP_0002_NODE_VERSION;
        assert!(minor > 0, "test assumes MIP-0002 ships in a minor release");
        v(&format!("MAIN.{}.{}", major, minor - 1))
    }

    #[test]
    fn same_major_required() {
        let (store, before, active_at) = store_with_mip_0002_active();
        let ours = mip_0002_release();
        let (major, _) = MIP_0002_NODE_VERSION;
        for now in [before, active_at] {
            assert!(!is_peer_version_compatible(
                &store,
                &ours,
                &v(&format!("MAIN.{}.0", major + 1)),
                now
            ));
            assert!(!is_peer_version_compatible(
                &store,
                &ours,
                &v(&format!("MAIN.{}.9", major - 1)),
                now
            ));
            let (_, minor) = MIP_0002_NODE_VERSION;
            assert!(!is_peer_version_compatible(
                &store,
                &ours,
                &v(&format!("TEST.{}.{}", major, minor)),
                now
            ));
        }
    }

    #[test]
    fn older_peers_accepted_until_mip_0002_is_active() {
        let (store, before, active_at) = store_with_mip_0002_active();
        let ours = mip_0002_release();
        let older = pre_mip_0002_release();

        assert!(is_peer_version_compatible(&store, &ours, &older, before));
        assert!(!is_peer_version_compatible(
            &store, &ours, &older, active_at
        ));
        assert!(is_peer_version_compatible(&store, &ours, &ours, active_at));
    }

    #[test]
    fn node_without_mip_0002_release_keeps_same_major_rule() {
        let (store, _, active_at) = store_with_mip_0002_active();
        let older = pre_mip_0002_release();

        assert!(is_peer_version_compatible(
            &store, &older, &older, active_at
        ));
        assert!(is_peer_version_compatible(
            &store,
            &older,
            &mip_0002_release(),
            active_at
        ));
    }
}
