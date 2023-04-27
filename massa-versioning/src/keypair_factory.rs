use massa_signature::KeyPair;

use crate::{
    versioning::{MipComponent, MipStore},
    versioning_factory::{FactoryError, VersioningFactory},
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
}
