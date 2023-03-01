use massa_time::MassaTime;
use std::collections::BTreeMap;
use std::iter;
use thiserror::Error;

use crate::versioning::{
    VersioningComponent, VersioningState, VersioningStateTypeId, VersioningStore,
};

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

#[derive(Clone)]
/// Strategy to use when creating a new object from a factory
pub enum FactoryStrategy {
    /// use get_best_version (see Factory trait)
    Best,
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

    /// Return the VersioningComponent associated with return type (Output)
    /// e.g. if type Output = Address, should return VersioningComponent::Address
    fn get_component() -> VersioningComponent;
    /// Access to the VersioningStore
    fn get_versioning_store(&self) -> VersioningStore;

    /// Get best version (aka last active for factory component)
    fn get_best_version(&self) -> u32 {
        let component = Self::get_component();
        let vi_store_ = self.get_versioning_store();
        let vi_store = vi_store_.0.read();
        let state_active = VersioningState::active();

        vi_store
            .0
            .iter()
            .rev()
            .find_map(|(vi, vsh)| {
                (vi.component == component && vsh.state == state_active).then_some(vi.version)
            })
            .unwrap_or(0)
    }

    /// Get best version at given timestamp (e.g. slot)
    fn get_best_version_at(&self, ts: MassaTime) -> Result<u32, FactoryError> {
        let component = Self::get_component();
        let vi_store_ = self.get_versioning_store();
        let vi_store = vi_store_.0.read();
        let state_active = VersioningState::active();

        // Iter backward, filter component + state active,
        let version = vi_store
            .0
            .iter()
            .rev()
            .filter(|(vi, vsh)| vi.component == component && vsh.state == state_active)
            .find_map(|(vi, vsh)| {
                let res = vsh.state_at(ts, vi.start, vi.timeout);
                match res {
                    Ok(VersioningStateTypeId::Active) => Some(vi.version),
                    _ => None,
                }
            })
            .unwrap_or(0);

        Ok(version)
    }

    /// Get all versions in 'Active state' for the associated VersioningComponent
    fn get_versions(&self) -> Vec<u32> {
        let component = Self::get_component();
        let vi_store_ = self.get_versioning_store();
        let vi_store = vi_store_.0.read();

        let state_active = VersioningState::active();
        let versions_iter = vi_store.0.iter().filter_map(|(vi, vsh)| {
            (vi.component == component && vsh.state == state_active).then_some(vi.version)
        });
        let versions: Vec<u32> = iter::once(0).chain(versions_iter).collect();
        versions
    }

    /// Get all versions for the associated VersioningComponent
    fn get_all_versions(&self) -> BTreeMap<u32, VersioningStateTypeId> {
        let component = Self::get_component();
        let vi_store_ = self.get_versioning_store();
        let vi_store = vi_store_.0.read();

        let versions_iter = vi_store.0.iter().filter_map(|(vi, vsh)| {
            (vi.component == component)
                .then_some((vi.version, VersioningStateTypeId::from(&vsh.state)))
        });
        iter::once((0, VersioningStateTypeId::Active))
            .chain(versions_iter)
            .collect()
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

    use std::collections::BTreeMap;
    use std::sync::Arc;

    use parking_lot::RwLock;

    use crate::test_exports::versioning_helpers::advance_state_until;
    use crate::versioning::{VersioningInfo, VersioningStateHistory, VersioningStoreRaw};
    use massa_time::MassaTime;

    // Define a struct Address with 2 versions AddressV0 & AddressV1
    #[derive(Debug)]
    struct AddressV0 {
        hash: String,
    }

    impl AddressV0 {
        fn new(hash: String) -> Self {
            Self { hash }
        }
    }

    #[derive(Debug)]
    struct AddressV1 {
        slot: String,
        creator: String,
        index: u32,
    }

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
        versioning_store: VersioningStore,
    }

    impl VersioningFactory for AddressFactory {
        type Output = Address;
        type Error = FactoryError;
        type Arguments = AddressArgs;

        fn get_component() -> VersioningComponent {
            VersioningComponent::Address
        }

        fn get_versioning_store(&self) -> VersioningStore {
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
                    match self.get_all_versions().get(&v) {
                        Some(s) if *s == VersioningStateTypeId::Active => Ok(v),
                        Some(s) if *s != VersioningStateTypeId::Active => {
                            Err(FactoryError::OnStateNotReady(v))
                        }
                        _ => Err(FactoryError::UnknownVersion(v)),
                    }
                }
                Some(FactoryStrategy::At(ts)) => self.get_best_version_at(ts),
                None | Some(FactoryStrategy::Best) => Ok(self.get_best_version()),
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
        let vi_1 = VersioningInfo {
            name: "MIP-0002".to_string(),
            version: 1,
            component: VersioningComponent::Address,
            start: MassaTime::from(12),
            timeout: MassaTime::from(15),
        };
        let vs_1 = VersioningStateHistory::new(MassaTime::from(10));

        let vi_2 = VersioningInfo {
            name: "MIP-0003".to_string(),
            version: 2,
            component: VersioningComponent::Address,
            start: MassaTime::from(25),
            timeout: MassaTime::from(28),
        };
        let vs_2 = VersioningStateHistory::new(MassaTime::from(18));

        let info = BTreeMap::from([(vi_1.clone(), vs_1), (vi_2.clone(), vs_2.clone())]);
        let vs_raw = VersioningStoreRaw(info);
        let vs = VersioningStore {
            0: Arc::new(RwLock::new(vs_raw)),
        };

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

        assert_eq!(fa.get_versions(), vec![0]);
        assert_eq!(fa.get_best_version(), 0);

        let addr_a = fa.create(&args, None);
        assert!(matches!(addr_a, Ok(Address::V0(_))));
        //
        // Version 2 is unknown
        let addr_ = fa.create(&args, Some(2.into()));
        assert!(matches!(addr_, Err(FactoryError::OnStateNotReady(2))));

        // Advance state 1 to Active
        let vs_1_new = advance_state_until(VersioningState::active(), &vi_1);
        // Create a new factory
        let info = BTreeMap::from([(vi_1.clone(), vs_1_new.clone()), (vi_2.clone(), vs_2)]);
        // Update versioning store
        vs.0.write().0 = info;

        assert_eq!(fa.get_versions(), vec![0, 1]);
        assert_eq!(
            fa.get_all_versions().keys().cloned().collect::<Vec<u32>>(),
            vec![0, 1, 2]
        );
        assert_eq!(fa.get_best_version(), 1);
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

        let vi_1 = VersioningInfo {
            name: "MIP-0002".to_string(),
            version: 1,
            component: VersioningComponent::Address,
            start: MassaTime::from(12),
            timeout: MassaTime::from(15),
        };
        let vs_1 = advance_state_until(VersioningState::active(), &vi_1);

        let vi_2 = VersioningInfo {
            name: "MIP-0003".to_string(),
            version: 2,
            component: VersioningComponent::Address,
            start: MassaTime::from(25),
            timeout: MassaTime::from(28),
        };
        let vs_2 = VersioningStateHistory::new(MassaTime::from(18));

        let info = BTreeMap::from([(vi_1.clone(), vs_1.clone()), (vi_2.clone(), vs_2.clone())]);
        let vs_raw = VersioningStoreRaw(info);
        let vs = VersioningStore {
            0: Arc::new(RwLock::new(vs_raw)),
        };

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
        let st_2 = FactoryStrategy::At(MassaTime::from(16)); // vi_1 is active (after vi_1.timeout)
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
        let vs_2_new = advance_state_until(VersioningState::active(), &vi_2);
        let info = BTreeMap::from([(vi_1.clone(), vs_1), (vi_2.clone(), vs_2_new)]);
        // Update versioning store
        vs.0.write().0 = info;

        assert_eq!(fa.get_versions(), vec![0, 1, 2]);
        let addr_st_4 = fa.create(&args, Some(st_4));
        // Version 2 is selected but this is not implemented in factory yet
        assert!(matches!(
            addr_st_4,
            Err(FactoryError::UnimplementedVersion(2))
        ));
    }
}
