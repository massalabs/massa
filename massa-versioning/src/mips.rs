#[allow(unused_imports)]
use std::collections::BTreeMap;

#[allow(unused_imports)]
use massa_time::MassaTime;

#[allow(unused_imports)]
use crate::versioning::{MipComponent, MipInfo, MipState};

#[cfg(not(feature = "test-exports"))]
pub fn get_mip_list() -> [(MipInfo, MipState); 2] {
    // When the MIP becomes defined, e.g. when merged to main branch
    let defined = MassaTime::from_utc_ymd_hms(2025, 5, 12, 10, 0, 0).unwrap(); // Monday 12th May 2025 10:00:00 UTC

    // TODO(release): the WMAS bytecode patch MIP (MIP-0002) below uses
    // placeholder future dates so it cannot activate prematurely. Before
    // enabling, set real `defined`/`start`/`timeout` dates AND fill in
    // `WMAS_ADDRESS` and `resources/wmas_patched.wasm` in massa-execution-worker.
    let defined_wmas = MassaTime::from_utc_ymd_hms(2026, 9, 1, 10, 0, 0).unwrap(); // TODO(release)

    let mip_list = [
        (
            MipInfo {
                name: "MIP-0001-DeferredCalls-And-Execution-BugFix".to_string(),
                version: 1,
                components: BTreeMap::from([
                    (MipComponent::Execution, 1),
                    (MipComponent::FinalState, 1),
                ]),
                start: MassaTime::from_utc_ymd_hms(2025, 5, 19, 10, 0, 0).unwrap(), // Monday 19th May 2025 10:00:00 UTC
                timeout: MassaTime::from_utc_ymd_hms(2025, 6, 19, 10, 0, 0).unwrap(), // Thursday 19th June 2025 10:00:00 UTC
                activation_delay: MassaTime::from_millis(7 * 24 * 60 * 60 * 1000),    // 7 days
            },
            MipState::new(defined),
        ),
        (
            MipInfo {
                name: "MIP-0002-WMAS-BytecodePatch".to_string(),
                version: 2,
                // Bumps MipComponent::Execution to 2, which MUST match
                // `massa_execution_worker::wmas_patch::WMAS_PATCH_EXEC_VERSION`.
                // Only the Execution component is bumped: the patch is applied
                // through normal ledger changes and does not alter the final
                // state format, so FinalState is not bumped.
                components: BTreeMap::from([(MipComponent::Execution, 2)]),
                // TODO(release): placeholder future dates, see note above.
                start: MassaTime::from_utc_ymd_hms(2026, 9, 8, 10, 0, 0).unwrap(),
                timeout: MassaTime::from_utc_ymd_hms(2026, 10, 8, 10, 0, 0).unwrap(),
                activation_delay: MassaTime::from_millis(7 * 24 * 60 * 60 * 1000), // 7 days
            },
            MipState::new(defined_wmas),
        ),
    ];

    // debug!("MIP list: {:?}", mip_list);
    #[allow(clippy::let_and_return)]
    mip_list
}

#[cfg(feature = "test-exports")]
pub fn get_mip_list() -> [(MipInfo, MipState); 1] {
    use crate::{
        test_helpers::versioning_helpers::advance_state_until,
        versioning::{Active, ComponentState},
    };

    println!("Running with test-exports feature");

    let mip_info_1 = MipInfo {
        name: "MIP-0001-DeferredCalls-And-Execution-BugFix".to_string(),
        version: 1,
        components: BTreeMap::from([(MipComponent::Execution, 1), (MipComponent::FinalState, 1)]),
        start: MassaTime::from_millis(2),
        timeout: MassaTime::from_millis(10),
        activation_delay: MassaTime::from_millis(2),
    };
    let mip_state_1 = advance_state_until(
        ComponentState::Active(Active {
            at: MassaTime::from_millis(3),
        }),
        &mip_info_1,
    );

    let mip_list = [(mip_info_1, mip_state_1)];

    println!("MIP list: {:?}", mip_list);
    #[allow(clippy::let_and_return)]
    mip_list
}
