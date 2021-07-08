// Copyright (c) 2021 MASSA LABS <info@massa.net>

pub use log::{debug, error, info, trace, warn};

#[macro_export]
macro_rules! massa_trace {
    ($evt:expr, $params:tt) => {
        log::trace!(
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
