use crate::{
    versioning::{ComponentStateTypeId, MipComponent, MipStore},
    versioning_factory::{FactoryError, FactoryStrategy, VersioningFactory},
};
use massa_hash::Hash;
use massa_models::address::{
    Address, SCAddress, SCAddressV0, SCAddressV1, UserAddress, UserAddressV0, UserAddressV1,
};

struct AddressFactory {
    versioning_store: MipStore,
}

enum AddressArgs {
    User { hash: Hash },
    SC { hash: Hash },
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
            Some(FactoryStrategy::Exact(v)) => match self.get_all_component_versions().get(&v) {
                Some(s) if *s == ComponentStateTypeId::Active => Ok(v),
                Some(s) if *s != ComponentStateTypeId::Active => {
                    Err(FactoryError::OnStateNotReady(v))
                }
                _ => Err(FactoryError::UnknownVersion(v)),
            },
            Some(FactoryStrategy::At(ts)) => self.get_latest_component_version_at(ts),
            None | Some(FactoryStrategy::Latest) => Ok(self.get_latest_component_version()),
        }?;

        let output: Address = match version {
            0 => match args {
                AddressArgs::User { hash } => {
                    Address::User(UserAddress::UserAddressV0(UserAddressV0(*hash)))
                }
                AddressArgs::SC { hash } => Address::SC(SCAddress::SCAddressV0(SCAddressV0(*hash))),
            },
            1 => match args {
                AddressArgs::User { hash } => {
                    Address::User(UserAddress::UserAddressV1(UserAddressV1(*hash)))
                }
                AddressArgs::SC { hash } => Address::SC(SCAddress::SCAddressV1(SCAddressV1(*hash))),
            },
            v => return Err(FactoryError::UnimplementedVersion(v)),
        };

        Ok(output)
    }
}
