// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Log utilities

#![warn(missing_docs)]

pub use serde_json;
pub use tracing;

#[macro_export]
/// tracing with some context
macro_rules! massa_trace {
    ($evt:expr, $params:tt) => {
        $crate::tracing::trace!("massa:{}:{}", $evt, $crate::serde_json::json!($params));
    };
}
