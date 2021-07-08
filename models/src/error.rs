use thiserror::Error;

#[derive(Error, Debug)]
pub enum ModelsError {
    #[error("Header is not hashable")]
    HeaderhashError,
}
