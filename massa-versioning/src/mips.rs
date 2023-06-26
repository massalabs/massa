#[allow(unused_imports)]
use std::collections::BTreeMap;

#[allow(unused_imports)]
use massa_time::MassaTime;

#[allow(unused_imports)]
use crate::versioning::{MipComponent, MipInfo, MipState};

pub fn get_mip_list() -> [(MipInfo, MipState); 0] {
    // placeholder
    let mip_list = [
        /*
        (MipInfo {
            name: "MIP-0000".to_string(),
            version: 0,
            components: BTreeMap::from([
                (MipComponent::Address, 0),
                (MipComponent::KeyPair, 0),
            ]),
            start: MassaTime::from_millis(0),
            timeout: MassaTime::from_millis(0),
            activation_delay: MassaTime::from_millis(0),
        },
        MipState::new(MassaTime::from_millis(0)))
        */
    ];

    // debug!("MIP list: {:?}", mip_list);
    mip_list
}
