// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General description
//! MIP = Massa Improvement proposal (similar to Bitcoin Improvement Proposal - BIP or Ethereum - EIP)
//!
//! MIPInfo -> represent a MIP (name, versions, time ranges)
//! MIPState -> Deployment state of a MIPInfo
//! MIPStore -> A map of MIPInfo -> MipState
//!
//! # Note on MipInfo versions
//!
//! There is 2 different versions:
//! * version -> Network version -> This is the network version to announce and thus to store in block header
//! * component_version -> This is the version for the associated component and is used in VersioningFactory
//!
//! # Note on MipState
//!
//! TODO
//!
//! # Advancing MipState
//!
//! The Versioning middleware is designed to process all finalized blocks and update the corresponding state
//! **It is not yet implemented**
//!
//! # VersioningFactory
//!
//! A Factory trait is there to ease the development of factory for Versioned component (e.g. address, block)
//!
//! All factories should query MIPStore in order to create a component with correct version; default implementation
//! are provided by the trait to avoid re writing theses query functions.
//!
//! Unit tests in versioning_factory.rs shows a basic but realistic implementation of a AddressFactory (impl the Factory trait)

pub mod versioning;
pub mod versioning_factory;

/// Test utils
#[cfg(feature = "testing")]
pub mod test_helpers;
