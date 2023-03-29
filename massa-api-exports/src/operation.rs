// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    block_id::BlockId,
    operation::{OperationId, SecureShareOperation},
};

use massa_signature::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

use crate::{display_if_true, display_option_bool};

/// operation input
#[derive(Serialize, Deserialize, Debug)]
pub struct OperationInput {
    /// The public key of the creator of the TX
    pub creator_public_key: PublicKey,
    /// The signature of the operation
    pub signature: Signature,
    /// The serialized version of the content `base58` encoded
    pub serialized_content: Vec<u8>,
}

/// Operation and contextual info about it
#[derive(Debug, Deserialize, Serialize)]
pub struct OperationInfo {
    /// id
    pub id: OperationId,
    /// true if operation is still in pool
    pub in_pool: bool,
    /// the operation appears in `in_blocks`
    /// if it appears in multiple blocks, these blocks are in different cliques
    pub in_blocks: Vec<BlockId>,
    /// true if the operation is final (for example in a final block)
    pub is_operation_final: Option<bool>,
    /// Thread in which the operation can be included
    pub thread: u8,
    /// the operation itself
    pub operation: SecureShareOperation,
    /// true if the operation execution succeeded, false if failed, None means unknown
    pub op_exec_status: Option<bool>,
}

impl std::fmt::Display for OperationInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Operation {}{}{}{}",
            self.id,
            display_if_true(self.in_pool, "in pool"),
            display_option_bool(
                self.is_operation_final,
                "operation is final",
                "operation is not final",
                "finality unkown"
            ),
            display_option_bool(self.op_exec_status, "succes", "failed", "status unkown")
        )?;
        writeln!(f, "In blocks:")?;
        for block_id in &self.in_blocks {
            writeln!(f, "\t- {}", block_id)?;
        }
        writeln!(f, "{}", self.operation)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use jsonrpsee_core::__reexports::serde_json::{self, Value};
    use massa_models::{amount::Amount, operation::OperationType};
    use serial_test::serial;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    #[test]
    #[serial]
    fn test_execute_sc_with_datastore() {
        let given_op = OperationType::ExecuteSC {
            max_gas: 123,
            max_coins: Amount::from_str("5000000").unwrap(),
            data: vec![23u8, 123u8, 44u8],
            datastore: BTreeMap::from([
                (vec![1, 2, 3], vec![4, 5, 6, 7, 8, 9]),
                (vec![22, 33, 44, 55, 66, 77], vec![11]),
                (vec![2, 3, 4, 5, 6, 7], vec![1]),
            ]),
        };

        let op_json_str = serde_json::to_string(&given_op).unwrap();

        let op_json_value: Value = serde_json::from_str(&op_json_str).unwrap();
        let datastore = op_json_value["ExecuteSC"]
            .as_object()
            .unwrap()
            .get("datastore")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(datastore.len(), 3);
        let first_entry = datastore[0].as_array().unwrap();
        assert_eq!(first_entry.len(), 2);
        let first_key = first_entry[0].as_array().unwrap();
        let first_value = first_entry[1].as_array().unwrap();
        assert_eq!(first_key.len(), 3);
        assert_eq!(first_value.len(), 6);

        let expected_op = serde_json::from_str(&op_json_str).unwrap();
        assert_eq!(given_op, expected_op);
    }
}
