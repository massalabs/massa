#[allow(unused_imports)]
use std::collections::BTreeMap;

#[allow(unused_imports)]
use massa_time::MassaTime;

#[allow(unused_imports)]
use crate::versioning::{MipComponent, MipInfo, MipState};

#[cfg(not(feature = "test-exports"))]
pub fn get_mip_list() -> [(MipInfo, MipState); 1] {
    // When the MIP becomes defined, e.g. when merged to main branch
    let defined = MassaTime::from_utc_ymd_hms(2025, 5, 7, 10, 0, 0).unwrap(); // Wednesday 7th May 2025 10:00:00 UTC

    let mip_list = [(
        MipInfo {
            name: "MIP-0001-DeferredCalls-And-Execution-BugFix".to_string(),
            version: 1,
            components: BTreeMap::from([
                (MipComponent::Execution, 1),
                (MipComponent::FinalState, 1),
            ]),
            start: MassaTime::from_utc_ymd_hms(2025, 5, 14, 10, 0, 0).unwrap(), // Wednesday 14th May 2025 10:00:00 UTC
            timeout: MassaTime::from_utc_ymd_hms(2025, 6, 14, 10, 0, 0).unwrap(), // Saturday 14th June 2025 10:00:00 UTC
            activation_delay: MassaTime::from_millis(7 * 24 * 60 * 60 * 1000),    // 7 days
        },
        MipState::new(defined),
    )];

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
