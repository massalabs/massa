use signature::{PublicKey, Signature};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct PubkeySig {
    pub public_key: PublicKey,
    pub signature: Signature,
}
