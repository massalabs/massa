pub use log::{debug, error, info, trace, warn};

macro_rules! function_path {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        &name[..name.len() - 3]
    }};
}

macro_rules! massa_trace {
    ($evt:expr, $params:tt) => {
        log::trace!(
            "massa_trace:{}",
            serde_json::json!({
                "origin": function_path!(),
                "event": $evt,
                "parameters": $params
            })
            .to_string()
        );
    };
}
