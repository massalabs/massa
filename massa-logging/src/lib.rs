// Copyright (c) 2022 MASSA LABS <info@massa.net>

#[macro_export]
macro_rules! massa_trace {
    ($evt:expr, $params:tt) => {
        tracing::trace!("massa:{}:{}", $evt, serde_json::json!($params));
    };
}
