use massa_signature::KeyPair;

use crate::{
    versioning::{MipComponent, MipStore},
    versioning_factory::{FactoryError, FactoryStrategy, VersioningFactory},
};

#[derive(Clone)]
pub struct KeyPairFactory {
    pub mip_store: MipStore,
}

impl VersioningFactory for KeyPairFactory {
    type Output = KeyPair;
    type Error = FactoryError;
    type Arguments = ();

    fn get_component() -> MipComponent {
        MipComponent::KeyPair
    }

    fn get_versioning_store(&self) -> MipStore {
        self.mip_store.clone()
    }

    fn create(
        &self,
        _args: &Self::Arguments,
        strategy: FactoryStrategy,
    ) -> Result<Self::Output, Self::Error> {
        let version = self.get_component_version_with_strategy(strategy)?;

        let output = KeyPair::generate(version.into())
            .map_err(|_| FactoryError::UnimplementedVersion(version))?;

        Ok(output)
    }
}
