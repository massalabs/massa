#[allow(unused_imports)]
use std::collections::BTreeMap;

#[allow(unused_imports)]
use massa_time::MassaTime;

#[allow(unused_imports)]
use crate::versioning::{MipComponent, MipInfo, MipState};

#[cfg(not(feature = "test-exports"))]
pub fn get_mip_list() -> [(MipInfo, MipState); 1] {
    let mip_list = [(
        MipInfo {
            name: "MIP-0001-Execution-BugFix-And-DeferredCalls".to_string(),
            version: 1,
            components: BTreeMap::from([
                (MipComponent::Execution, 1),
                (MipComponent::FinalState, 1),
                (MipComponent::Execution, 1),
                (MipComponent::FinalState, 1),
            ]),
            // Note: All bootstrap servers should have been updated to the latest version before the start time
            start: MassaTime::from_utc_ymd_hms(2025, 01, 13, 14, 0, 0).unwrap(), // Monday, January 13, 2025 2:00:00 PM UTC
            // Give 1 week for the MIP to be accepted
            timeout: MassaTime::from_utc_ymd_hms(2025, 01, 20, 14, 0, 0).unwrap(), // Monday, January 20, 2025 2:00:00 PM UTC
            activation_delay: MassaTime::from_millis(1 * 60 * 60 * 1000), // The MIP will be activated 1 hour after the LockedIn state
        },
        MipState::new(MassaTime::from_utc_ymd_hms(2025, 01, 10, 10, 0, 0).unwrap()), // Friday, January 10, 2025 10:00:00 AM UTC
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
        name: "MIP-0001-Execution-BugFix".to_string(),
        version: 1,
        components: BTreeMap::from([(MipComponent::Execution, 1), (MipComponent::FinalState, 1)]),
        start: MassaTime::from_millis(2), // TODO: set when known, ex: MassaTime::from_utc_ymd_hms(2024, 7, 10, 15, 0, 0).unwrap();
        timeout: MassaTime::from_millis(10), // TODO: set when known
        activation_delay: MassaTime::from_millis(2), // TODO: set when known, 3 days as an example
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
