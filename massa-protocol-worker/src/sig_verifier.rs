// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Optimized batch signature verifier for operations

use massa_hash::Hash;
use massa_protocol_exports::ProtocolError;
use massa_signature::{verify_signature_batch, PublicKey, Signature};
use rayon::prelude::*;

/// Efficiently verifies a batch of signatures in parallel.
/// Returns an error if at least one of them fails to verify.
pub fn verify_sigs_batch(ops: &[(Hash, Signature, PublicKey)]) -> Result<(), ProtocolError> {
    // nothing to verify => OK
    if ops.len() == 0 {
        return Ok(());
    }

    // normal verif is fastest for 1 verif
    if ops.len() == 1 {
        let (hash, signature, public_key) = ops[0];
        return public_key
            .verify_signature(&hash, &signature)
            .map_err(|_err| ProtocolError::WrongSignature);
    }

    // single-core batch verif is fastest until 2 verifs
    if ops.len() <= 2 {
        return massa_signature::verify_signature_batch(ops)
            .map_err(|_err| ProtocolError::WrongSignature);
    }

    // otherwise, use parallel batch verif

    // compute chunk size for parallelization
    let chunk_size = std::cmp::max(1, ops.len() / rayon::current_num_threads());
    // process chunks in parallel
    ops.par_chunks(chunk_size)
        .try_for_each(|chunk| verify_signature_batch(&chunk))
        .map_err(|_err| ProtocolError::WrongSignature)
}
