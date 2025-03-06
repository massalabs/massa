#[allow(unused_imports)]
use std::collections::BTreeMap;

#[allow(unused_imports)]
use massa_time::MassaTime;

#[allow(unused_imports)]
use crate::versioning::{MipComponent, MipInfo, MipState};

pub fn get_mip_list() -> [(MipInfo, MipState); 1] {
    let mip_list = [(
        MipInfo {
            name: "MIP-0001-Execution-BugFix-And-DeferredCalls".to_string(),
            version: 1,
            components: BTreeMap::from([]),
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
