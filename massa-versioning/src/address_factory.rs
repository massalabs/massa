use crate::{
    versioning::{MipComponent, MipStore},
    versioning_factory::{FactoryError, FactoryStrategy, VersioningFactory},
};
use massa_hash::Hash;
use massa_models::address::{
    Address, SCAddress, SCAddressV0, SCAddressV1, UserAddress, UserAddressV0, UserAddressV1,
};

#[derive(Clone)]
pub struct AddressFactory {
    pub mip_store: MipStore,
}

pub enum AddressArgs {
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
        self.mip_store.clone()
    }

    fn create(
        &self,
        args: &Self::Arguments,
        strategy: FactoryStrategy,
    ) -> Result<Self::Output, Self::Error> {
        let version = self.get_component_version_with_strategy(strategy)?;

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
