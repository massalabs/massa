use thiserror::Error;

use crate::versioning::VersioningComponent;

/// Factory error
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum FactoryError {
    #[error("Unknown version, cannot build obj with version: {0}")]
    UnknownVersion(u32),
    #[error("Version: {0} is in versioning store but not yet implemented")]
    UnimplementedVersion(u32), // programmer error, forgot to implement new version in factory traits
}

/// Strategy to use when creating a new object from a factory
pub enum FactoryStrategy {
    /// use get_best_version (see Factory trait)
    Best,
    /// Require to create an object with this specific version
    Exact(u32),
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
    /// e.g. if type Ouput = Address, should return VersioningComponent::Address
    fn get_component() -> VersioningComponent;
    /// Access to the VersioningStore
    // fn get_versioning_store(&self) -> VersioningStore;

    /// Get best version
    fn get_best_version(&self) -> u32 {
        todo!()
    }
    /// Get all active versions
    fn get_versions(&self) -> Vec<u32> {
        todo!()
    }

    /// Create
    fn create(
        &self,
        args: Self::Arguments,
        strategy: Option<FactoryStrategy>,
    ) -> Result<Self::Output, Self::Error>;
}

#[cfg(test)]
mod test {
    use super::*;
    // use parking_lot::RwLock;
    // use std::collections::BTreeMap;
    // use std::sync::Arc;

    use crate::versioning::VersioningInfo;

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
        // versioning_store: VersioningStore,
    }

    impl VersioningFactory for AddressFactory {
        type Output = Address;
        type Error = FactoryError;
        type Arguments = AddressArgs;

        fn get_component() -> VersioningComponent {
            VersioningComponent::Address
        }

        /*
        fn get_versioning_store(&self) -> VersioningStore {
            self.versioning_store.clone()
        }
        */

        fn create(
            &self,
            args: Self::Arguments,
            strategy: Option<FactoryStrategy>,
        ) -> Result<Self::Output, Self::Error> {
            Ok(Address::V0(AddressV0 {
                hash: args.hash.unwrap(),
            }))
        }
    }

    #[test]
    fn test_address_factory_args() {
        /*
        let vsi_sca1 = VersioningInfo {
            name: "MIP-0002".to_string(),
            version: 1,
            component: VersioningComponent::Address,
            start: Default::default(),
            timeout: Default::default(),
        };

        let vsi_blk1 = VersioningInfo {
            name: "MIP-0003".to_string(),
            version: 1,
            component: VersioningComponent::Block,
            start: Default::default(),
            timeout: Default::default(),
        };

        let info = BTreeMap::from([
            (vsi_sca1.clone(), VersioningState::defined()),
            (vsi_blk1.clone(), VersioningState::active()),
        ]);
        let vs_raw = VersioningStoreRaw { data: info };
        let vs = VersioningStore {
            0: Arc::new(RwLock::new(vs_raw)),
        };
        */

        let fa = AddressFactory {
            // versioning_store: vs.clone(),
        };

        // TODO: impl Default with None everywhere
        let args = AddressArgs {
            hash: Some("sdofjsklfhskfjl".into()),
            slot: None,
            creator: None,
            index: None,
        };

        let addr_a = fa.create(args, None);
        assert!(addr_a.is_ok());
        assert!(matches!(addr_a, Ok(Address::V0(_))));
        println!("addr_a: {:?}", addr_a);

        // TODO: next
    }
}
