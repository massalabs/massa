use signature::{PublicKey, Signature};

use serde::{Deserialize, Serialize};

use crate::hhasher::PreHashed;

#[derive(Debug, Serialize, Deserialize)]
pub struct PubkeySig {
    pub public_key: PublicKey,
    pub signature: Signature,
}

impl PreHashed for massa_hash::hash::Hash {}
