#[allow(unused_imports)]
use std::collections::BTreeMap;

#[allow(unused_imports)]
use massa_time::MassaTime;

#[allow(unused_imports)]
use crate::versioning::{MipComponent, MipInfo, MipState};

#[cfg(not(feature = "test-exports"))]
pub fn get_mip_list() -> [(MipInfo, MipState); 2] {
    // When the MIPs becomes defined, e.g. when merged to main branch
    let defined_1 = MassaTime::from_utc_ymd_hms(2025, 5, 12, 10, 0, 0).unwrap(); // Monday 12th May 2025 10:00:00 UTC
    let defined_2 = MassaTime::from_utc_ymd_hms(2026, 8, 3, 10, 0, 0).unwrap(); // Monday 3rd August 2026 10:00:00 UTC

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
            MipState::new(defined_1),
        ),
        (
            MipInfo {
                name: "MIP-0002-BugFix".to_string(),
                version: 2,
                components: BTreeMap::from([
                    (MipComponent::Execution, 2),
                    (MipComponent::FinalState, 2),
                ]),
                start: MassaTime::from_utc_ymd_hms(2026, 8, 10, 10, 0, 0).unwrap(), // Monday 10th August 2026 10:00:00 UTC
                timeout: MassaTime::from_utc_ymd_hms(2026, 9, 9, 10, 0, 0).unwrap(), // Wednesday 9th September 2026 10:00:00 UTC
                activation_delay: MassaTime::from_millis(7 * 24 * 60 * 60 * 1000),   // 7 days
            },
            MipState::new(defined_2),
        ),
    ];

    // debug!("MIP list: {:?}", mip_list);
    #[allow(clippy::let_and_return)]
    mip_list
}

#[cfg(feature = "test-exports")]
pub fn get_mip_list() -> [(MipInfo, MipState); 2] {
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

    let mip_info_2 = MipInfo {
        name: "MIP-0002-BugFix".to_string(),
        version: 2,
        components: BTreeMap::from([(MipComponent::Execution, 2), (MipComponent::FinalState, 2)]),
        start: MassaTime::from_millis(12),
        timeout: MassaTime::from_millis(20),
        activation_delay: MassaTime::from_millis(2),
    };
    let mip_state_2 = advance_state_until(
        ComponentState::Active(Active {
            at: MassaTime::from_millis(13),
        }),
        &mip_info_2,
    );

    let mip_list = [(mip_info_1, mip_state_1), (mip_info_2, mip_state_2)];

    println!("MIP list: {:?}", mip_list);
    #[allow(clippy::let_and_return)]
    mip_list
}
