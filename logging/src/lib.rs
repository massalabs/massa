// Copyright (c) 2021 MASSA LABS <info@massa.net>

#[macro_export]
macro_rules! massa_trace {
    ($evt:expr, $params:tt) => {
        tracing::trace!(
            "massa_trace:{}",
            serde_json::json!({
                // "origin": function_path!(),
                "event": $evt,
                "parameters": $params
            })
            .to_string()
        );
    };
}
