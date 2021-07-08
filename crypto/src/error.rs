use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Hash parse error")]
    HashParseError(String),

    #[error("Signature parse error")]
    SignatureParseError(String),

    #[error("PublicKey parse error")]
    PublicKeyParseError(String),

    #[error("PrivateKey parse error")]
    PrivateKeyParseError(String),

    #[error("message parse error : {0}")]
    MessageParseError(#[from] secp256k1::Error),
}
