use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Hash parse error : {0}")]
    HashParseError(String),

    #[error("Signature parse error : {0}")]
    SignatureParseError(String),

    #[error("PublicKey parse error : {0}")]
    PublicKeyParseError(String),

    #[error("PrivateKey parse error : {0}")]
    PrivateKeyParseError(String),

    #[error("message parse error : {0}")]
    MessageParseError(#[from] secp256k1::Error),
}
