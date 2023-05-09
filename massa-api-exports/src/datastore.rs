// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::address::Address;
use serde::{Deserialize, Serialize};

/// Datastore entry query input structure
#[derive(Debug, Deserialize, Clone, Serialize)]
pub(crate)  struct DatastoreEntryInput {
    /// associated address of the entry
    pub(crate)  address: Address,
    /// datastore key
    pub(crate)  key: Vec<u8>,
}

/// Datastore entry query output structure
#[derive(Debug, Deserialize, Clone, Serialize)]
pub(crate)  struct DatastoreEntryOutput {
    /// final datastore entry value
    pub(crate)  final_value: Option<Vec<u8>>,
    /// candidate datastore entry value
    pub(crate)  candidate_value: Option<Vec<u8>>,
}

impl std::fmt::Display for DatastoreEntryOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "final value: {:?}", self.final_value)?;
        writeln!(f, "candidate value: {:?}", self.candidate_value)?;
        Ok(())
    }
}
