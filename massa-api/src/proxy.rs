//! Middleware that proxies requests at a specified URI to internal
//! RPC method calls.

use hyper::header::{ACCEPT, CONTENT_TYPE};
use hyper::http::HeaderValue;
use hyper::{Body, Method, Request, Response, StatusCode, Uri};
use jsonrpsee::core::error::Error as RpcError;
use std::error::Error;
use tower::{Layer, Service};
// use std::error::Error;
use jsonrpsee::core::params::ArrayParams;
use jsonrpsee::core::traits::ToRpcParams;
use jsonrpsee::types::{Id, RequestSer};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use urlencoding::decode;

pub const URI_ADDRESSES: &str = "/addresses";
pub const URI_BLOCKS: &str = "/blocks";
pub const URI_ENDORSEMENTS: &str = "/endorsements";
pub const URI_OPERATIONS: &str = "/operations";

/// Layer that applies [`MassaProxyGetRequest`] which proxies the `GET /path` requests to
/// specific RPC method calls and that strips the response.
///
/// See [`MassaProxyGetRequest`] for more details.
#[derive(Debug, Clone)]
pub struct MassaProxyGetRequestLayer {}

impl MassaProxyGetRequestLayer {
    /// Creates a new [`ProxyGetRequestLayer`].
    ///
    /// See [`MassaProxyGetRequest`] for more details.
    pub fn new() -> Result<Self, RpcError> {
        Ok(Self {})
    }
}

impl<S> Layer<S> for MassaProxyGetRequestLayer {
    type Service = MassaProxyGetRequest<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MassaProxyGetRequest::new(inner)
            .expect("Path already validated in ProxyGetRequestLayer; qed")
    }
}

/// Proxy `GET /path` requests to the specified RPC method calls.
///
/// # Request
///
/// The `GET /path` requests are modified into valid `POST` requests for
/// calling the RPC method. This middleware adds appropriate headers to the
/// request, and completely modifies the request `BODY`.
///
/// # Response
///
/// The response of the RPC method is stripped down to contain only the method's
/// response, removing any RPC 2.0 spec logic regarding the response' body.
#[derive(Debug, Clone)]
pub struct MassaProxyGetRequest<S> {
    inner: S,
}

impl<S> MassaProxyGetRequest<S> {
    /// Creates a new [`MassaProxyGetRequest`].
    ///
    /// The request `GET /path` is redirected to the provided method.
    /// Fails if the path does not start with `/`.
    pub fn new(inner: S) -> Result<Self, RpcError> {
        Ok(Self { inner })
    }
}

impl<S> Service<Request<Body>> for MassaProxyGetRequest<S>
where
    S: Service<Request<Body>, Response = Response<Body>>,
    S::Response: 'static,
    S::Error: Into<Box<dyn Error + Send + Sync>> + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = Box<dyn Error + Send + Sync + 'static>;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let uri = req.uri();
        let path = uri.path();

        let mut modify = false;
        let mut params = ArrayParams::new();

        if req.method() == Method::GET
            && (path.eq(URI_ADDRESSES)
                || path.eq(URI_BLOCKS)
                || path.eq(URI_ENDORSEMENTS)
                || path.eq(URI_OPERATIONS))
        {
            if let Some(query) = req.uri().query() {
                // If this line is reached
                // Extract params list from query params

                let decoded_query = decode(query)
                    .expect(&*format!("Error when decode query for GET {}", path))
                    .to_string();

                for param in decoded_query.split('&') {
                    // example param for addresses:"ids=["Address1", "Address2"]"
                    let kv: Vec<&str> = param.splitn(2, '=').collect();
                    if kv.len() == 2 {
                        let value: Value = serde_json::from_str(kv[1]).unwrap_or_else(|_| {
                            panic!("Error when deserialize params query for GET {}", path)
                        });
                        params
                            .insert(value)
                            .expect("Error when insert into ArrayParams");
                        modify = true;
                    }
                }
            }
        }

        // Proxy the request to the appropriate method call.
        if modify {
            let method = match path {
                URI_ADDRESSES => "get_addresses",
                URI_BLOCKS => "get_blocks",
                URI_OPERATIONS => "get_operations",
                URI_ENDORSEMENTS => "get_endorsements",
                _ => "",
            };

            // RPC methods are accessed with `POST`.
            *req.method_mut() = Method::POST;
            // Precautionary remove the URI.
            *req.uri_mut() = Uri::from_static("/");

            // Requests must have the following headers:
            req.headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            req.headers_mut()
                .insert(ACCEPT, HeaderValue::from_static("application/json"));

            // Adjust the body to reflect the method call.
            let rpc_params = params.to_rpc_params().unwrap();
            let body = Body::from(
                serde_json::to_string(&RequestSer::owned(Id::Number(0), method, rpc_params))
                    .expect("Valid request; qed"),
            );

            req = req.map(|_| body);
        }

        // Call the inner service and get a future that resolves to the response.
        let fut = self.inner.call(req);

        // Adjust the response if needed.
        let res_fut = async move {
            let res = fut.await.map_err(|err| err.into())?;

            // Nothing to modify: return the response as is.
            if !modify {
                return Ok(res);
            }

            let body = res.into_body();
            let bytes = hyper::body::to_bytes(body).await?;

            #[derive(serde::Deserialize, Debug)]
            struct RpcPayload<'a> {
                #[serde(borrow)]
                result: &'a serde_json::value::RawValue,
            }

            let response: Response<Body> =
                if let Ok(payload) = serde_json::from_slice::<RpcPayload>(&bytes) {
                    hyper::Response::builder()
                        .status(StatusCode::OK)
                        .header(
                            "content-type",
                            HeaderValue::from_static("application/json; charset=utf-8"),
                        )
                        .body(Body::from(payload.result.to_string()))
                        .expect("Unable to parse response body for type conversion")
                } else {
                    let error = serde_json::to_string(&jsonrpsee::types::ErrorResponse::borrowed(
                        jsonrpsee::types::error::ErrorCode::InternalError.into(),
                        Id::Null,
                    ))
                    .expect("built from known-good data; qed");
                    hyper::Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header(
                            "content-type",
                            HeaderValue::from_static("application/json; charset=utf-8"),
                        )
                        .body(error.into())
                        // Parsing `StatusCode` and `HeaderValue` is infalliable but
                        // parsing body content is not.
                        .expect("Unable to parse response body for type conversion")
                };

            Ok(response)
        };

        Box::pin(res_fut)
    }
}
