use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("failed retrieving storage controller event")]
    ControllerEventError,
}
