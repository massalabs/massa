// Copyright (c) 2024 MASSA LABS <info@massa.net>

//! `mockall_wrap` is a small proc-macro companion to [`mockall`].
//!
//! `mockall::automock` generates a mock struct where each instance owns its
//! own expectations. If the mock is cloned, the two copies do not share state.
//! This is a problem for Massa controller traits that are used as
//! `Box<dyn Trait>` and cloned at runtime: tests want every clone to count
//! toward the same expectations.
//!
//! This crate provides the `#[mockall_wrap::wrap]` attribute. When placed right
//! before `#[mockall::automock]` on a trait, it generates a
//! `Mock{Trait}Wrapper` struct that holds the mock behind an `Arc` (or an
//! `Arc<RwLock<…>>` if the trait has `&mut self` methods), delegates every
//! trait method to the shared inner mock, and treats `clone_box()` specially so
//! that it returns a boxed clone of the wrapper instead of triggering a mock
//! expectation.
//!
//! Example:
//!
//! ```ignore
//! #[mockall_wrap::wrap]
//! #[mockall::automock]
//! pub trait MyController: Send + Sync {
//!     fn work(&self) -> i32;
//!     fn clone_box(&self) -> Box<dyn MyController>;
//! }
//!
//! let mut wrapper = MockMyControllerWrapper::new();
//! wrapper.set_expectations(|mock| {
//!     mock.expect_work().returning(|| 42);
//! });
//! let clone: Box<dyn MyController> = wrapper.clone_box();
//! assert_eq!(clone.work(), 42);
//! ```

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ItemTrait, TraitItem};

/// Returns `true` if the trait declares at least one `&mut self` method.
fn trait_has_mut_self(trait_block: &ItemTrait) -> bool {
    trait_block.items.iter().any(|item| {
        if let TraitItem::Fn(method) = item {
            method.sig.inputs.first().is_some_and(|first| {
                matches!(
                    first,
                    FnArg::Receiver(receiver) if receiver.mutability.is_some()
                )
            })
        } else {
            false
        }
    })
}

/// Generates the wrapper struct, its `Clone` impl, and the trait impl.
fn generate_wrap(trait_block: &ItemTrait, has_mut: bool) -> TokenStream {
    let trait_name = &trait_block.ident;
    let mock_name = format_ident!("Mock{}", trait_name);
    let wrapper_name = format_ident!("Mock{}Wrapper", trait_name);

    let inner_type = if has_mut {
        quote! { ::std::sync::Arc<::std::sync::RwLock<#mock_name>> }
    } else {
        quote! { ::std::sync::Arc<#mock_name> }
    };

    let init_code = if has_mut {
        quote! { ::std::sync::Arc::new(::std::sync::RwLock::new(#mock_name::new())) }
    } else {
        quote! { ::std::sync::Arc::new(#mock_name::new()) }
    };

    let set_expectations = if has_mut {
        quote! {
            /// Configures the shared inner mock.
            ///
            /// # Panics
            ///
            /// Panics if the wrapper has already been cloned (the `Arc` must have
            /// a single strong reference when expectations are configured).
            pub fn set_expectations<F: FnOnce(&mut #mock_name)>(&mut self, f: F) {
                f(&mut ::std::sync::Arc::get_mut(&mut self.inner).unwrap().write().unwrap());
            }
        }
    } else {
        quote! {
            /// Configures the shared inner mock.
            ///
            /// # Panics
            ///
            /// Panics if the wrapper has already been cloned (the `Arc` must have
            /// a single strong reference when expectations are configured).
            pub fn set_expectations<F: FnOnce(&mut #mock_name)>(&mut self, f: F) {
                f(::std::sync::Arc::get_mut(&mut self.inner).unwrap());
            }
        }
    };

    let methods: Vec<TokenStream> = trait_block
        .items
        .iter()
        .filter_map(|item| {
            let TraitItem::Fn(method) = item else {
                return None;
            };

            let signature = &method.sig;
            let method_name = &signature.ident;
            let params: Vec<_> = signature
                .inputs
                .iter()
                .filter_map(|arg| match arg {
                    FnArg::Typed(pat_type) => {
                        let pat = &pat_type.pat;
                        Some(quote! { #pat })
                    }
                    FnArg::Receiver(_) => None,
                })
                .collect();

            let receiver = signature.inputs.first().and_then(|first| match first {
                FnArg::Receiver(receiver) => Some(receiver.mutability.is_some()),
                FnArg::Typed(_) => None,
            });

            // `clone_box` is special: the real implementation is meant to clone
            // the *trait object*, not to be an expectation on the mock itself.
            if method_name == "clone_box" {
                return Some(quote! {
                    #signature {
                        Box::new(self.clone())
                    }
                });
            }

            let body = match receiver {
                None => {
                    // Static method: delegate to the mock type directly.
                    quote! { #mock_name::#method_name(#(#params),*) }
                }
                Some(true) if has_mut => {
                    quote! { self.inner.write().unwrap().#method_name(#(#params),*) }
                }
                Some(false) if has_mut => {
                    quote! { self.inner.read().unwrap().#method_name(#(#params),*) }
                }
                Some(_) => {
                    quote! { self.inner.#method_name(#(#params),*) }
                }
            };

            Some(quote! {
                #signature {
                    #body
                }
            })
        })
        .collect();

    quote! {
        /// A wrapper around a [`mockall`] mock that makes cloned instances share
        /// the same expectations.
        pub struct #wrapper_name {
            inner: #inner_type,
        }

        impl ::std::clone::Clone for #wrapper_name {
            fn clone(&self) -> Self {
                Self {
                    inner: ::std::clone::Clone::clone(&self.inner),
                }
            }
        }

        impl #wrapper_name {
            /// Creates a new wrapper around a fresh mock.
            pub fn new() -> Self {
                Self {
                    inner: #init_code,
                }
            }

            #set_expectations
        }

        impl ::std::default::Default for #wrapper_name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl #trait_name for #wrapper_name {
            #(#methods)*
        }
    }
}

/// Attribute macro that wraps a `#[mockall::automock]` trait so that its mock
/// instances share expectations across clones.
#[proc_macro_attribute]
pub fn wrap(
    _attrs: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let input: TokenStream = input.into();
    let trait_block = syn::parse2::<ItemTrait>(input.clone())
        .expect("#[mockall_wrap::wrap] can only be applied to a trait");

    let has_mut = trait_has_mut_self(&trait_block);
    let generated = generate_wrap(&trait_block, has_mut);

    let output = quote! {
        #input
        #generated
    };

    output.into()
}
