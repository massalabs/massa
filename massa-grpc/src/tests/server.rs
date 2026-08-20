// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::server::check_mtls_requires_tls;
use crate::tests::mock::grpc_public_service;

#[test]
fn test_mtls_requires_tls() {
    let addr = "[::]:2222".parse().unwrap();
    let mut config = grpc_public_service(&addr).grpc_config;

    // plaintext, the default: accepted
    config.enable_tls = false;
    config.enable_mtls = false;
    assert!(check_mtls_requires_tls(&config).is_ok());

    // mTLS on top of TLS: accepted
    config.enable_tls = true;
    config.enable_mtls = true;
    assert!(check_mtls_requires_tls(&config).is_ok());

    // TLS without mTLS: accepted
    config.enable_tls = true;
    config.enable_mtls = false;
    assert!(check_mtls_requires_tls(&config).is_ok());

    // mTLS without TLS: rejected, this would silently serve in plaintext
    config.enable_tls = false;
    config.enable_mtls = true;
    let err = check_mtls_requires_tls(&config).unwrap_err();
    assert!(err
        .to_string()
        .contains("`enable_mtls` requires `enable_tls`"));
}
