use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("parsing error : {0}")]
    ParsingError(String),

    #[error("error forwarded by engine: {0}")]
    EngineError(#[from] secp256k1::Error),
}
