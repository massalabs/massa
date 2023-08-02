// Copyright (c) 2023 MASSA LABS <info@massa.net>

//! This module provides functions for generating certificates for a Certificate Authority (CA) and
//! signing certificates using the CA certificate in mutual TLS (mTLS) scenarios.
//!
//! The `gen_cert_for_ca` function generates a certificate for the CA, while the `gen_signed_cert`
//! function generates a certificate signed by the CA. These functions utilize the `rcgen` crate for
//! generating and managing certificates.

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, CertificateSigningRequest, DistinguishedName,
    DnType, IsCa, KeyUsagePurpose, RcgenError, PKCS_ECDSA_P256_SHA256,
};
use std::collections::HashSet;

/// Generate a certificate for a certificate authority (CA).
///
/// # Returns
///
/// Returns a `Certificate` representing the generated CA certificate, or an `RcgenError` if there was an error during the certificate generation.
pub fn gen_cert_for_ca() -> Result<Certificate, RcgenError> {
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Auto-Generated Massalabs CA");

    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.alg = &PKCS_ECDSA_P256_SHA256;
    params.distinguished_name = dn;
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];

    let ca_cert = Certificate::from_params(params)?;
    Ok(ca_cert)
}

/// Generate a certificate signed by a certificate authority (CA).
///
/// # Arguments
///
/// * `ca`: A reference to the CA certificate used to sign the generated certificate.
/// * `subject_alt_names`: A vector of subject alternative names to include in the certificate.
///
/// # Returns
///
/// Returns a tuple containing the signed certificate PEM and the corresponding private key PEM,
/// or an `RcgenError` if there was an error during the certificate generation or signing process.
pub fn gen_signed_cert(
    ca: &Certificate,
    subject_alt_names: Vec<String>,
) -> Result<(String, String), RcgenError> {
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Auto-Generated Massa gRPC Server");

    // Add "localhost" to the subject alternative names
    let all_subject_alt_names_set: HashSet<String> = subject_alt_names
        .into_iter()
        .chain(vec!["localhost".to_string()])
        .collect();
    let all_subject_alt_names = all_subject_alt_names_set.into_iter().collect::<Vec<_>>();
    let mut params = CertificateParams::new(all_subject_alt_names);
    params.is_ca = IsCa::NoCa;
    params.alg = &PKCS_ECDSA_P256_SHA256;
    params.distinguished_name = dn;

    let unsigned = Certificate::from_params(params)?;
    let request_pem = unsigned.serialize_request_pem()?;
    let csr = CertificateSigningRequest::from_pem(&request_pem)?;
    let signed_pem = csr.serialize_pem_with_signer(ca)?;

    Ok((signed_pem, unsigned.serialize_private_key_pem()))
}
