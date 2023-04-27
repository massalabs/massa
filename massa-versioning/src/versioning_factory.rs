use massa_time::MassaTime;
use std::collections::BTreeMap;
use std::iter;
use thiserror::Error;

use crate::versioning::{ComponentState, ComponentStateTypeId, MipComponent, MipStore};

/// Factory error
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum FactoryError {
    #[error("Unknown version, cannot build obj with version: {0}")]
    UnknownVersion(u32),
    #[error("Version: {0} is in versioning store but not yet implemented")]
    UnimplementedVersion(u32), // programmer error, forgot to implement new version in factory traits
    #[error("Version: {0} is not active yet")]
    OnStateNotReady(u32),
    #[error("Could not create object of type {0}: {1}")]
    OnCreate(String, String),
}

#[derive(Clone, Debug)]
/// Strategy to use when creating a new object from a factory
pub enum FactoryStrategy {
    /// use get_latest_version (see Factory trait)
    Latest,
    /// Require to create an object with this specific version
    Exact(u32),
    /// Create an object given a timestamp (e.g slot)
    At(MassaTime),
}

impl From<u32> for FactoryStrategy {
    fn from(value: u32) -> Self {
        FactoryStrategy::Exact(value)
    }
}

// Factory traits

/// Trait for Factory that create objects based on versioning store
pub trait VersioningFactory {
    /// Factory will create only one object of a given type,
    /// so a FactoryAddress (impl VersioningFactory), will have type Output = Address
    type Output;
    /// Error if object cannot be created (+ used for VersioningFactoryArgs)
    type Error;
    /// Arguments struct in order to create Self::Output
    type Arguments;

    /// Return the MipComponent associated with return type (Output)
    /// e.g. if type Output = Address, should return MipComponent::Address
    fn get_component() -> MipComponent;
    /// Access to the MipStore
    fn get_versioning_store(&self) -> MipStore;

    /// Get latest component version (aka last active for the factory component)
    fn get_latest_component_version(&self) -> u32 {
        let component = Self::get_component();
        let vi_store_ = self.get_versioning_store();
        let vi_store = vi_store_.0.read();

        vi_store
            .store
            .iter()
            .rev()
            .find_map(|(vi, vsh)| {
                if matches!(vsh.state, ComponentState::Active(_)) {
                    vi.components.get(&component).copied()
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }

    /// Get latest version at given timestamp (e.g. slot)
    fn get_latest_component_version_at(&self, ts: MassaTime) -> Result<u32, FactoryError> {
        let component = Self::get_component();
        let vi_store_ = self.get_versioning_store();
        let vi_store = vi_store_.0.read();

        // Iter backward, filter component + state active,
        let version = vi_store
            .store
            .iter()
            .rev()
            .filter(|(vi, vsh)| {
                vi.components.get(&component).is_some()
                    && matches!(vsh.state, ComponentState::Active(_))
            })
            .find_map(|(vi, vsh)| {
                let res = vsh.state_at(ts, vi.start, vi.timeout);
                match res {
                    Ok(ComponentStateTypeId::Active) => vi.components.get(&component).copied(),
                    _ => None,
                }
            })
            .unwrap_or(0);

        Ok(version)
    }

    /// Get all versions in 'Active state' for the associated MipComponent
    fn get_all_active_component_versions(&self) -> Vec<u32> {
        let component = Self::get_component();
        let vi_store_ = self.get_versioning_store();
        let vi_store = vi_store_.0.read();

        let versions_iter = vi_store.store.iter().filter_map(|(vi, vsh)| {
            if matches!(vsh.state, ComponentState::Active(_)) {
                vi.components.get(&component).copied()
            } else {
                None
            }
        });
        let versions: Vec<u32> = iter::once(0).chain(versions_iter).collect();
        versions
    }

    /// Get all versions (at any state) for the associated MipComponent
    fn get_all_component_versions(&self) -> BTreeMap<u32, ComponentStateTypeId> {
        let component = Self::get_component();
        let vi_store_ = self.get_versioning_store();
        let vi_store = vi_store_.0.read();

        let versions_iter = vi_store.store.iter().filter_map(|(vi, vsh)| {
            vi.components
                .get(&component)
                .copied()
                .map(|component_version| {
                    (component_version, ComponentStateTypeId::from(&vsh.state))
                })
        });
        iter::once((0, ComponentStateTypeId::Active))
            .chain(versions_iter)
            .collect()
    }

    /// Get the version the current component with the given startegy
    fn get_component_version_with_strategy(
        &self,
        strategy: Option<FactoryStrategy>,
    ) -> Result<u32, FactoryError> {
        match strategy {
            Some(FactoryStrategy::Exact(v)) => match self.get_all_component_versions().get(&v) {
                Some(s) if *s == ComponentStateTypeId::Active => Ok(v),
                Some(s) if *s != ComponentStateTypeId::Active => {
                    Err(FactoryError::OnStateNotReady(v))
                }
                _ => Err(FactoryError::UnknownVersion(v)),
            },
            Some(FactoryStrategy::At(ts)) => self.get_latest_component_version_at(ts),
            None | Some(FactoryStrategy::Latest) => Ok(self.get_latest_component_version()),
        }
    }

    /// Create an object of type Self::Output
    fn create(
        &self,
        args: &Self::Arguments,
        strategy: Option<FactoryStrategy>,
    ) -> Result<Self::Output, Self::Error>;
}

#[cfg(test)]
mod test {
    use super::*;

    use std::collections::{BTreeMap, HashMap};

    use crate::test_helpers::versioning_helpers::advance_state_until;
    use crate::versioning::{MipInfo, MipState, MipStatsConfig};

    use massa_time::MassaTime;

    // Define a struct Address with 2 versions AddressV0 & AddressV1
    #[allow(dead_code)]
    #[derive(Debug)]
    struct AddressV0 {
        hash: String,
    }

    #[allow(dead_code)]
    impl AddressV0 {
        fn new(hash: String) -> Self {
            Self { hash }
        }
    }

    #[allow(dead_code)]
    #[derive(Debug)]
    struct AddressV1 {
        slot: String,
        creator: String,
        index: u32,
    }

    #[allow(dead_code)]
    impl AddressV1 {
        fn new(slot: String, creator: String, index: u32) -> Self {
            Self {
                slot,
                creator,
                index,
            }
        }
    }

    #[derive(Debug)]
    enum Address {
        V0(AddressV0),
        V1(AddressV1),
    }

    //
    struct AddressArgs {
        // V0
        hash: Option<String>,
        // V1
        slot: Option<String>,
        creator: Option<String>,
        index: Option<u32>,
    }

    // Now we define an Address factory

    #[derive(Debug)]
    struct AddressFactory {
        versioning_store: MipStore,
    }

    impl VersioningFactory for AddressFactory {
        type Output = Address;
        type Error = FactoryError;
        type Arguments = AddressArgs;

        fn get_component() -> MipComponent {
            MipComponent::Address
        }

        fn get_versioning_store(&self) -> MipStore {
            self.versioning_store.clone()
        }

        fn create(
            &self,
            args: &Self::Arguments,
            strategy: Option<FactoryStrategy>,
        ) -> Result<Self::Output, Self::Error> {
            let version = match strategy {
                Some(FactoryStrategy::Exact(v)) => {
                    // This is not optimal - can use get_versions and return a less descriptive error
                    match self.get_all_component_versions().get(&v) {
                        Some(s) if *s == ComponentStateTypeId::Active => Ok(v),
                        Some(s) if *s != ComponentStateTypeId::Active => {
                            Err(FactoryError::OnStateNotReady(v))
                        }
                        _ => Err(FactoryError::UnknownVersion(v)),
                    }
                }
                Some(FactoryStrategy::At(ts)) => self.get_latest_component_version_at(ts),
                None | Some(FactoryStrategy::Latest) => Ok(self.get_latest_component_version()),
            };

            match version {
                Ok(0) => Ok(Address::V0(AddressV0 {
                    hash: args.hash.clone().ok_or(FactoryError::OnCreate(
                        stringify!(Self::Output).to_string(),
                        "Please provide hash in args".to_string(),
                    ))?,
                })),
                Ok(1) => Ok(Address::V1(AddressV1 {
                    slot: args.slot.clone().ok_or(FactoryError::OnCreate(
                        stringify!(Self::Output).to_string(),
                        "Please provide 'slot' in args".to_string(),
                    ))?,
                    creator: args.creator.clone().unwrap(),
                    index: args.index.clone().unwrap(),
                })),
                Ok(v) => Err(FactoryError::UnimplementedVersion(v)),
                Err(e) => Err(e),
            }
        }
    }

    #[test]
    fn test_address_factory_args() {
        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 1,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from(12),
            timeout: MassaTime::from(15),
            activation_delay: MassaTime::from(2),
        };
        let vs_1 = MipState::new(MassaTime::from(10));

        let vi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from(25),
            timeout: MassaTime::from(28),
            activation_delay: MassaTime::from(2),
        };
        let vs_2 = MipState::new(MassaTime::from(18));

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };

        let vs = MipStore::try_from((
            [(vi_1.clone(), vs_1), (vi_2.clone(), vs_2.clone())],
            mip_stats_cfg,
        ))
        .unwrap();
        let fa = AddressFactory {
            versioning_store: vs.clone(),
        };

        let args = AddressArgs {
            hash: Some("sdofjsklfhskfjl".into()),
            slot: Some("slot_4_2".to_string()),
            creator: Some("me_pubk".to_string()),
            index: Some(3),
        };
        let args_no_v1 = AddressArgs {
            hash: Some("sdofjsklfhskfjl".into()),
            slot: None,
            creator: Some("me_pubk".to_string()),
            index: Some(3),
        };

        assert_eq!(fa.get_all_active_component_versions(), vec![0]);
        assert_eq!(fa.get_latest_component_version(), 0);

        let addr_a = fa.create(&args, None);
        assert!(matches!(addr_a, Ok(Address::V0(_))));
        //
        // Version 2 is unknown
        let addr_ = fa.create(&args, Some(2.into()));
        assert!(matches!(addr_, Err(FactoryError::OnStateNotReady(2))));

        // Advance state 1 to Active
        let vs_1_new = advance_state_until(ComponentState::active(), &vi_1);
        // Create a new factory
        let info = BTreeMap::from([(vi_1.clone(), vs_1_new.clone()), (vi_2.clone(), vs_2)]);
        // Update versioning store
        vs.0.write().store = info;

        assert_eq!(fa.get_all_active_component_versions(), vec![0, 1]);
        assert_eq!(
            fa.get_all_component_versions()
                .keys()
                .cloned()
                .collect::<Vec<u32>>(),
            vec![0, 1, 2]
        );
        assert_eq!(fa.get_latest_component_version(), 1);
        let addr_b = fa.create(&args, None);
        assert!(matches!(addr_b, Ok(Address::V1(_))));

        // Error if not enough args
        let addr_ = fa.create(&args_no_v1, Some(1.into()));
        assert!(matches!(addr_, Err(FactoryError::OnCreate(_, _))));

        // Can still create AddressV0
        let addr_c = fa.create(&args, Some(0.into()));
        println!("addr_c: {:?}", addr_c);
        assert!(matches!(addr_c, Ok(Address::V0(_))));
    }

    #[test]
    fn test_factory_strategy_at() {
        // Test factory & FactoryStrategy::At(...)

        let vi_1 = MipInfo {
            name: "MIP-0002".to_string(),
            version: 1,
            components: HashMap::from([(MipComponent::Address, 1)]),
            start: MassaTime::from(12),
            timeout: MassaTime::from(15),
            activation_delay: MassaTime::from(2),
        };
        let vs_1 = advance_state_until(ComponentState::active(), &vi_1);

        let vi_2 = MipInfo {
            name: "MIP-0003".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 2)]),
            start: MassaTime::from(25),
            timeout: MassaTime::from(28),
            activation_delay: MassaTime::from(2),
        };
        let vs_2 = MipState::new(MassaTime::from(18));

        let mip_stats_cfg = MipStatsConfig {
            block_count_considered: 10,
            counters_max: 5,
        };

        let vs = MipStore::try_from((
            [(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())],
            mip_stats_cfg,
        ))
        .unwrap();

        let fa = AddressFactory {
            versioning_store: vs.clone(),
        };

        let args = AddressArgs {
            hash: Some("sdofjsklfhskfjl".into()),
            slot: Some("slot_4_2".to_string()),
            creator: Some("me_pubk".to_string()),
            index: Some(3),
        };

        //
        let st_1 = FactoryStrategy::At(MassaTime::from(8)); // vi_1 not yet defined
        let ts_1_2 = MassaTime::from(13);
        let st_1_2 = FactoryStrategy::At(ts_1_2); // vi_1 is started (after vi_1.start)
        let st_2 = FactoryStrategy::At(MassaTime::from(18)); // vi_1 is active (after start + activation delay)
        let st_3 = FactoryStrategy::At(MassaTime::from(27)); // vi_2 is started or locked_in
        let st_4 = FactoryStrategy::At(MassaTime::from(30)); // vi_2 is active (after vi_2.timeout)

        let addr_st_1 = fa.create(&args, Some(st_1));
        let addr_st_1_2 = fa.create(&args, Some(st_1_2.clone()));
        let addr_st_2 = fa.create(&args, Some(st_2));
        let addr_st_3 = fa.create(&args, Some(st_3));
        let addr_st_4 = fa.create(&args, Some(st_4.clone()));

        assert!(matches!(addr_st_1, Ok(Address::V0(_))));
        assert!(matches!(addr_st_1_2, Ok(Address::V0(_))));
        assert!(matches!(addr_st_2, Ok(Address::V1(_))));
        assert!(matches!(addr_st_3, Ok(Address::V1(_))));
        assert!(matches!(addr_st_4, Ok(Address::V1(_)))); // for now, vs_2 is not active yet

        // Advance state 2 to Active
        let vs_2_new = advance_state_until(ComponentState::active(), &vi_2);
        let info = BTreeMap::from([(vi_1.clone(), vs_1), (vi_2.clone(), vs_2_new)]);
        // Update versioning store
        vs.0.write().store = info;

        assert_eq!(fa.get_all_active_component_versions(), vec![0, 1, 2]);
        let addr_st_4 = fa.create(&args, Some(st_4));
        // Version 2 is selected but this is not implemented in factory yet
        assert!(matches!(
            addr_st_4,
            Err(FactoryError::UnimplementedVersion(2))
        ));
    }
}
