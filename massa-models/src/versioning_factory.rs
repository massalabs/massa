use crate::versioning::{Active, VersioningComponent, VersioningState, VersioningStore};
use std::collections::HashMap;
use thiserror::Error;

/// Factory error
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum FactoryError {
    #[error("Unknown version, cannot build obj with version: {0}")]
    UnknownVersion(u32),
    #[error("Version: {0} is in versioning store but not yet implemented")]
    UnimplementedVersion(u32), // programmer error, forgot to implement new version in factory traits
    #[error("factory value error: {0}")]
    FactoryValue(#[from] FactoryValueError),
}

/// Factory value error
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum FactoryValueError {
    #[error("Value is of type {0}, type {1} was expected")]
    BadType(String, String),
    #[error("Arguments list is empty")]
    Empty,
    #[error("Arguments map does not contains '{0}'")]
    UnknownKey(String),
    #[error("Not enough arguments in list, wants '{0}'")]
    NotEnoughArgs(String),
}

/// A generic type used for factories to pass arguments
#[allow(missing_docs)]
#[derive(Clone)]
pub enum FactoryValue {
    String(String),
    U32(u32),
}

impl TryInto<String> for &FactoryValue {
    type Error = FactoryValueError;
    fn try_into(self) -> Result<String, Self::Error> {
        match self {
            FactoryValue::String(s) => Ok(s.clone()),
            _ => Err(FactoryValueError::BadType(
                stringify!(u32).to_string(),
                stringify!(type_name_of_val(self)).to_string(),
            )),
        }
    }
}

impl TryInto<u32> for &FactoryValue {
    type Error = FactoryValueError;
    fn try_into(self) -> Result<u32, Self::Error> {
        match self {
            FactoryValue::U32(v) => Ok(*v),
            FactoryValue::String(_) => Err(FactoryValueError::BadType(
                stringify!(String).to_string(),
                stringify!(u32).to_string(),
            )),
        }
    }
}

impl From<String> for FactoryValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for FactoryValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<u32> for FactoryValue {
    fn from(value: u32) -> Self {
        Self::U32(value)
    }
}

// FactoryStrategy

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

    /// Return the VersioningComponent associated with return type (Output)
    /// e.g. if type Ouput = Address, should return VersioningComponent::Address
    fn get_component() -> VersioningComponent;
    /// Access to the VersioningStore
    fn get_versioning_store(&self) -> VersioningStore;
    /// Get best version
    fn get_best_version(&self) -> u32 {
        let mut versions = self.get_versions();
        versions.sort();
        // println!("versions: {:?}", versions);
        let res = versions.iter().next().or(Some(&0));
        return *res.unwrap();
    }
    /// Get all active versions
    fn get_versions(&self) -> Vec<u32> {
        let component = Self::get_component();
        let vi_store_ = self.get_versioning_store();
        let vi_store = vi_store_.0.read();

        let state_active = VersioningState::Active(Active::new());
        let versions: Vec<u32> = vi_store
            .data
            .iter()
            .filter(|(k, v)| k.component == component && **v == state_active)
            .map(|(k, _v)| k.version)
            .collect();
        return versions;
    }
}

/// Same as VersioningFactory but create object using generic arguments (provided as a list or hashmap)
pub trait VersioningFactoryArgs: VersioningFactory {
    /// Build Self::Item from list of arguments
    fn try_from_args(
        &self,
        args: &Vec<FactoryValue>,
        strategy: Option<FactoryStrategy>,
    ) -> Result<Self::Output, Self::Error>;

    /// Build Self::Item from Hashmap of arguments
    fn try_from_kwargs(
        &self,
        args: &HashMap<String, FactoryValue>,
        strategy: Option<FactoryStrategy>,
    ) -> Result<Self::Output, Self::Error>;
}

#[cfg(test)]
mod test {
    use super::*;
    use parking_lot::RwLock;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use crate::versioning::{Active, Defined, VersioningInfo, VersioningStoreRaw};

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

    impl TryFrom<&Vec<FactoryValue>> for AddressV0 {
        type Error = FactoryValueError;

        fn try_from(_kwargs: &Vec<FactoryValue>) -> Result<Self, Self::Error> {
            todo!()
        }
    }

    impl TryFrom<&HashMap<String, FactoryValue>> for AddressV0 {
        type Error = FactoryValueError;

        fn try_from(kwargs: &HashMap<String, FactoryValue>) -> Result<Self, Self::Error> {
            let hash = kwargs
                .get("hash")
                .ok_or(FactoryValueError::UnknownKey("hash".to_string()))?
                .try_into()?;
            Ok(AddressV0::new(hash))
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

    impl TryFrom<&Vec<FactoryValue>> for AddressV1 {
        type Error = FactoryValueError;

        fn try_from(args: &Vec<FactoryValue>) -> Result<Self, Self::Error> {
            let mut args_iter = args.iter();
            let slot = args_iter
                .next()
                .ok_or(FactoryValueError::Empty)?
                .try_into()?;
            let creator = args_iter
                .next()
                .ok_or(FactoryValueError::NotEnoughArgs("creator".to_string()))?
                .try_into()?;
            let index = args_iter
                .next()
                .ok_or(FactoryValueError::NotEnoughArgs("index".to_string()))?
                .try_into()?;
            Ok(AddressV1::new(slot, creator, index))
        }
    }

    impl TryFrom<&HashMap<String, FactoryValue>> for AddressV1 {
        type Error = FactoryError;

        fn try_from(kwargs: &HashMap<String, FactoryValue>) -> Result<Self, Self::Error> {
            let slot = kwargs
                .get("slot")
                .ok_or(FactoryValueError::UnknownKey("slot".to_string()))?
                .try_into()?;
            let creator = kwargs
                .get("creator")
                .ok_or(FactoryValueError::UnknownKey("creator".to_string()))?
                .try_into()?;
            let index = kwargs
                .get("index")
                .ok_or(FactoryValueError::UnknownKey("index".to_string()))?
                .try_into()?;

            Ok(AddressV1::new(slot, creator, index))
        }
    }

    #[derive(Debug)]
    enum Address {
        V0(AddressV0),
        V1(AddressV1),
    }

    // Now we define an Address factory
    // + implement VersioningFactoryArgs as we want to create from "Generic" args

    #[derive(Debug)]
    struct AddressFactory {
        versioning_store: VersioningStore,
    }

    impl VersioningFactory for AddressFactory {
        type Output = Address;
        type Error = FactoryError;

        fn get_component() -> VersioningComponent {
            VersioningComponent::Address
        }

        fn get_versioning_store(&self) -> VersioningStore {
            self.versioning_store.clone()
        }
    }

    impl VersioningFactoryArgs for AddressFactory {
        fn try_from_args(
            &self,
            args: &Vec<FactoryValue>,
            strategy: Option<FactoryStrategy>,
        ) -> Result<Self::Output, FactoryError> {
            let version = match strategy {
                Some(FactoryStrategy::Exact(v)) => {
                    if v == 0 || self.get_versions().contains(&v) {
                        Ok(v)
                    } else {
                        Err(FactoryError::UnknownVersion(v))
                    }
                }
                _ => Ok(self.get_best_version()),
            };

            match version {
                Ok(0) => Ok(Address::V0(
                    // SCAddressV0::try_from(args)?,
                    args.try_into().map_err(|e| FactoryError::from(e))?,
                )),
                Ok(1) => Ok(Address::V1(
                    // SCAddressV1::try_from(args)?,
                    args.try_into()?,
                )),
                Ok(v) => Err(FactoryError::UnimplementedVersion(v)),
                Err(e) => Err(e),
            }
        }

        fn try_from_kwargs(
            &self,
            kwargs: &HashMap<String, FactoryValue>,
            strategy: Option<FactoryStrategy>,
        ) -> Result<Self::Output, FactoryError> {
            // TODO: facto this in trait?
            let version = match strategy {
                Some(FactoryStrategy::Exact(v)) => {
                    if v == 0 || self.get_versions().contains(&v) {
                        Ok(v)
                    } else {
                        Err(FactoryError::UnknownVersion(v))
                    }
                }
                _ => Ok(self.get_best_version()),
            };

            match version {
                Ok(0) => Ok(Address::V0(kwargs.try_into()?)),
                Ok(1) => Ok(Address::V1(kwargs.try_into()?)),
                Ok(v) => Err(FactoryError::UnimplementedVersion(v)),
                Err(e) => Err(e),
            }
        }
    }

    #[test]
    fn test_address_factory_args() {
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
            (vsi_sca1.clone(), VersioningState::Defined(Defined::new())),
            (vsi_blk1.clone(), VersioningState::Active(Active::new())),
        ]);
        let vs_raw = VersioningStoreRaw { data: info };
        let vs = VersioningStore {
            0: Arc::new(RwLock::new(vs_raw)),
        };

        let fa = AddressFactory {
            versioning_store: vs.clone(),
        };

        let mut kwargs = HashMap::from([("hash".to_string(), "sdofjsklfhskfjl".into())]);

        let addr_a = fa.try_from_kwargs(&kwargs, None);
        assert!(addr_a.is_ok());
        assert!(matches!(addr_a, Ok(Address::V0(_))));
        println!("addr_a: {:?}", addr_a);

        // Trying to create an address version 1 (AddressV1) but it is not yet active
        // + note that arguments are still for version 0
        let addr_b = fa.try_from_kwargs(&kwargs, Some(FactoryStrategy::Exact(1)));
        assert!(addr_b.is_err());
        println!("addr_b: {:?}", addr_b);

        // Now make AddressV1 active and try again
        vs.clone()
            .0
            .write()
            .data
            .entry(vsi_sca1)
            .and_modify(|e| *e = VersioningState::Active(Active::new()));

        // Still an error but this times it's because arguments are not ok
        let addr_c = fa.try_from_kwargs(&kwargs, Some(FactoryStrategy::Exact(1)));
        assert!(addr_c.is_err());
        // Display an useful error message
        println!("addr_c: {}", addr_c.err().unwrap());

        kwargs.insert("slot".to_string(), "slot_0_1".into());
        kwargs.insert("creator".to_string(), "me_and_my_pub_key".into());
        kwargs.insert("index".to_string(), 1.into());

        // Now it's ok for AddressV1
        let addr_d = fa.try_from_kwargs(&kwargs, Some(FactoryStrategy::Exact(1)));
        assert!(addr_d.is_ok());

        // + Can still create an AddressV0
        let addr_e = fa.try_from_kwargs(&kwargs, Some(FactoryStrategy::Exact(0)));
        assert!(addr_e.is_ok());

        // Default will now create an AddressV1
        let addr_f = fa.try_from_kwargs(&kwargs, Some(FactoryStrategy::Exact(0)));
        assert!(addr_f.is_ok());
    }
}
