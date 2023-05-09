// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Optimized batch signature verifier

use massa_hash::Hash;
use massa_protocol_exports::ProtocolError;
use massa_signature::{verify_signature_batch, PublicKey, Signature};
use rayon::{prelude::ParallelIterator, slice::ParallelSlice};

//TODO: Benchmark
/// Limit for small batch optimization
const SMALL_BATCH_LIMIT: usize = 2;

/// Efficiently verifies a batch of signatures in parallel.
/// Returns an error if at least one of them fails to verify.
pub(crate)  fn verify_sigs_batch(ops: &[(Hash, Signature, PublicKey)]) -> Result<(), ProtocolError> {
    // if it's a small batch, use single-core verification
    if ops.len() <= SMALL_BATCH_LIMIT {
        return verify_signature_batch(ops).map_err(|_err| ProtocolError::WrongSignature);
    }

    // otherwise, use parallel batch verif

    // compute chunk size for parallelization
    let chunk_size = std::cmp::max(1, ops.len() / rayon::current_num_threads());
    // process chunks in parallel
    ops.par_chunks(chunk_size)
        .try_for_each(verify_signature_batch)
        .map_err(|_err| ProtocolError::WrongSignature)
}
