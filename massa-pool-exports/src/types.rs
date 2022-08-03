//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::cmp::Reverse;

use massa_models::OperationId;
use num::rational::Ratio;

type OperationCursorInner = (Reverse<Ratio<u64>>, OperationId);
/// A cursor for pool operations, sorted by increasing quality
#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct PoolOperationCursor(OperationCursorInner);

impl PoolOperationCursor {
    /// Create a new pool operation cursor based on inner data
    pub fn new(inner: OperationCursorInner) -> Self {
        Self(inner)
    }
}
