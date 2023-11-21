#[cfg(test)]
use crate::constants::{HASH_PARAMS, NONCE_SIZE, SALT_SIZE};
#[cfg(test)]
use crate::decrypt::decrypt;
#[cfg(test)]
use crate::encrypt::encrypt;

#[test]
fn test_encrypt() {
    let password = "password";
    let data = "data";

    let result = encrypt(password, data.as_bytes());
    assert!(result.is_ok());

    let cipher_data = result.unwrap();
    assert_eq!(
        cipher_data.encrypted_bytes.len(),
        HASH_PARAMS.output_length - NONCE_SIZE
    );
    assert_eq!(cipher_data.salt.len(), SALT_SIZE);
    assert_eq!(cipher_data.nonce.len(), NONCE_SIZE);
}

#[test]
fn test_encrypt_decrypt() {
    let password = "password";
    let data = "data";

    let cipher_data = encrypt(password, data.as_bytes()).unwrap();
    let result = decrypt(password, cipher_data);
    assert!(result.is_ok());

    let decrypted_data = result.unwrap();
    assert_eq!(decrypted_data, data.as_bytes());
}

#[test]
fn test_encrypt_decrypt_bad_password() {
    let data = "data";

    let cipher_data = encrypt("password", data.as_bytes()).unwrap();
    decrypt("wrong", cipher_data).expect_err("Wrong password should failed");
}
