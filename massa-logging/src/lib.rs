// Copyright (c) 2021 MASSA LABS <info@massa.net>

#[macro_export]
macro_rules! massa_trace {
    ($evt:expr, $params:tt) => {
        tracing::trace!(
            "massa_trace:{}:{}",
            $evt,
            // THIS IS FOR DEBUG PURPOSE ONLY
            |string: serde_json::Value| -> serde_json::Value {
                println!("##### TRACE WAS EVALUATED AT RUNTIME! #####");
                string
            }(serde_json::json!($params))
        );
    };
}
