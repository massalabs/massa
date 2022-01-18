// Copyright (c) 2021 MASSA LABS <info@massa.net>

#[macro_export]
macro_rules! massa_trace {
    ($evt:expr, $params:tt) => {
        tracing::trace!("massa_trace:{}:{}", $evt, serde_json::json!($params));
    };
}
