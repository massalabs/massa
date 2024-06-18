#[allow(unused_imports)]
use std::collections::BTreeMap;

#[allow(unused_imports)]
use massa_time::MassaTime;

#[allow(unused_imports)]
use crate::versioning::{MipComponent, MipInfo, MipState};

pub fn get_mip_list() -> [(MipInfo, MipState); 1] {
    let mip_list = [
        (
            MipInfo {
                name: "MIP-0001-ASC-BugFix".to_string(),
                version: 1,
                components: BTreeMap::from([(MipComponent::AscExecution, 1)]),
                start: MassaTime::from_millis(0), // TODO: set when known, ex: MassaTime::from_utc_ymd_hms(2024, 7, 10, 15, 0, 0).unwrap();
                timeout: MassaTime::from_millis(0), // TODO: set when known
                activation_delay: MassaTime::from_millis(3 * 24 * 60 * 60 * 1000), // TODO: set when known, 3 days as an example
            },
            MipState::new(MassaTime::from_millis(0)),
        ), // TODO: set when known, (when the MIP becomes defined, e.g. when merged to main branch)
    ];

    // debug!("MIP list: {:?}", mip_list);
    #[allow(clippy::let_and_return)]
    mip_list
}
