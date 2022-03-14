// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Log utilities

#![warn(missing_docs)]
#[macro_export]
/// tracing with some context
macro_rules! massa_trace {
    ($evt:expr, $params:tt) => {
        tracing::trace!("massa:{}:{}", $evt, serde_json::json!($params));
    };
}
