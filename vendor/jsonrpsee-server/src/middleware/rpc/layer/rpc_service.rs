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

//! JSON-RPC service middleware.

use super::ResponseFuture;
use std::sync::Arc;

use crate::middleware::rpc::RpcServiceT;
use crate::ConnectionId;
use futures_util::future::BoxFuture;
use jsonrpsee_core::server::{
	BoundedSubscriptions, MethodCallback, MethodResponse, MethodSink, Methods, SubscriptionState,
};
use jsonrpsee_core::traits::IdProvider;
use jsonrpsee_types::error::{reject_too_many_subscriptions, ErrorCode};
use jsonrpsee_types::{ErrorObject, Request};

/// JSON-RPC service middleware.
#[derive(Clone, Debug)]
pub struct RpcService {
	conn_id: ConnectionId,
	methods: Methods,
	max_response_body_size: usize,
	cfg: RpcServiceCfg,
}

/// Configuration of the RpcService.
#[derive(Clone, Debug)]
pub(crate) enum RpcServiceCfg {
	/// The server supports only calls.
	OnlyCalls,
	/// The server supports both method calls and subscriptions.
	CallsAndSubscriptions {
		bounded_subscriptions: BoundedSubscriptions,
		sink: MethodSink,
		id_provider: Arc<dyn IdProvider>,
		_pending_calls: tokio::sync::mpsc::Sender<()>,
	},
}

impl RpcService {
	/// Create a new service.
	pub(crate) fn new(
		methods: Methods,
		max_response_body_size: usize,
		conn_id: ConnectionId,
		cfg: RpcServiceCfg,
	) -> Self {
		Self { methods, max_response_body_size, conn_id, cfg }
	}
}

impl<'a> RpcServiceT<'a> for RpcService {
	// The rpc module is already boxing the futures and
	// it's used to under the hood by the RpcService.
	type Future = ResponseFuture<BoxFuture<'a, MethodResponse>>;

	fn call(&self, req: Request<'a>) -> Self::Future {
		let conn_id = self.conn_id;
		let max_response_body_size = self.max_response_body_size;

		let Request { id, method, params, extensions, .. } = req;
		let params = jsonrpsee_types::Params::new(params.as_ref().map(|p| serde_json::value::RawValue::get(p)));

		match self.methods.method_with_name(&method) {
			None => {
				let rp =
					MethodResponse::error(id, ErrorObject::from(ErrorCode::MethodNotFound)).with_extensions(extensions);
				ResponseFuture::ready(rp)
			}
			Some((_name, method)) => match method {
				MethodCallback::Async(callback) => {
					let params = params.into_owned();
					let id = id.into_owned();

					let fut = (callback)(id, params, conn_id, max_response_body_size, extensions);
					ResponseFuture::future(fut)
				}
				MethodCallback::Sync(callback) => {
					let rp = (callback)(id, params, max_response_body_size, extensions);
					ResponseFuture::ready(rp)
				}
				MethodCallback::Subscription(callback) => {
					let RpcServiceCfg::CallsAndSubscriptions {
						bounded_subscriptions,
						sink,
						id_provider,
						_pending_calls,
					} = self.cfg.clone()
					else {
						tracing::warn!("Subscriptions not supported");
						let rp = MethodResponse::error(id, ErrorObject::from(ErrorCode::InternalError))
							.with_extensions(extensions);
						return ResponseFuture::ready(rp);
					};

					if let Some(p) = bounded_subscriptions.acquire() {
						let conn_state =
							SubscriptionState { conn_id, id_provider: &*id_provider.clone(), subscription_permit: p };

						let fut = callback(id.clone(), params, sink, conn_state, extensions);
						ResponseFuture::future(fut)
					} else {
						let max = bounded_subscriptions.max();
						let rp =
							MethodResponse::error(id, reject_too_many_subscriptions(max)).with_extensions(extensions);
						ResponseFuture::ready(rp)
					}
				}
				MethodCallback::Unsubscription(callback) => {
					// Don't adhere to any resource or subscription limits; always let unsubscribing happen!

					let RpcServiceCfg::CallsAndSubscriptions { .. } = self.cfg else {
						tracing::warn!("Subscriptions not supported");
						let rp = MethodResponse::error(id, ErrorObject::from(ErrorCode::InternalError))
							.with_extensions(extensions);
						return ResponseFuture::ready(rp);
					};

					let rp = callback(id, params, conn_id, max_response_body_size, extensions);
					ResponseFuture::ready(rp)
				}
			},
		}
	}
}
