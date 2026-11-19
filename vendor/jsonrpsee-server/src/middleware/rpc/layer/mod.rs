// Copyright 2019-2021 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any
// person obtaining a copy of this software and associated
// documentation files (the "Software"), to deal in the
// Software without restriction, including without
// limitation the rights to use, copy, modify, merge,
// publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software
// is furnished to do so, subject to the following
// conditions:
//
// The above copyright notice and this permission notice
// shall be included in all copies or substantial portions
// of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
// ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
// PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
// SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
// IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! Specific middleware layer implementation provided by jsonrpsee.

pub mod either;
pub mod logger;
pub mod rpc_service;

pub use logger::*;
pub use rpc_service::*;

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::future::{Either, Future};
use jsonrpsee_core::server::MethodResponse;
use pin_project::pin_project;

/// Response which may be ready or a future.
#[derive(Debug)]
#[pin_project]
pub struct ResponseFuture<F>(#[pin] futures_util::future::Either<F, std::future::Ready<MethodResponse>>);

impl<F> ResponseFuture<F> {
	/// Returns a future that resolves to a response.
	pub fn future(f: F) -> ResponseFuture<F> {
		ResponseFuture(Either::Left(f))
	}

	/// Return a response which is already computed.
	pub fn ready(response: MethodResponse) -> ResponseFuture<F> {
		ResponseFuture(Either::Right(std::future::ready(response)))
	}
}

impl<F: Future<Output = MethodResponse>> Future for ResponseFuture<F> {
	type Output = MethodResponse;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.project().0.poll(cx)
	}
}
