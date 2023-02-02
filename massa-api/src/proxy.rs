//! Middleware that proxies requests at a specified URI to internal
//! RPC method calls.

use hyper::header::{ACCEPT, CONTENT_TYPE};
use hyper::http::HeaderValue;
use hyper::{Body, Method, Request, Response, StatusCode, Uri};
use jsonrpsee::core::error::Error as RpcError;
use jsonrpsee::core::params::ArrayParams;
use jsonrpsee::core::traits::ToRpcParams;
use jsonrpsee::types::{ErrorObject, ErrorResponse, Id, RequestSer};
use massa_api_exports::error::ApiError;
use serde_json::Value;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};
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
/// calling the RPC method. This middleware adds appropriate headers to the request
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
    pub fn new(inner: S) -> Result<Self, RpcError> {
        Ok(Self { inner })
    }
}

/// Transform HTTP Get request with query params to POST RPC
fn transform_request(req: &mut Request<Body>) -> Result<(), ApiError> {
    let path = req.uri().path();
    let mut params = ArrayParams::new();
    let mut modify = false;

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
                .map_err(|_e| ApiError::BadRequest("Unable to decode query".to_string()))?
                .to_string();

            for param in decoded_query.split('&') {
                // example param for addresses:"ids=["Address1", "Address2"]"
                let kv: Vec<&str> = param.splitn(2, '=').collect();
                if kv.len() == 2 {
                    let value: Value = serde_json::from_str(kv[1])
                        .map_err(|e| ApiError::InternalServerError(e.to_string()))?;
                    params
                        .insert(value)
                        .map_err(|e| ApiError::InternalServerError(e.to_string()))?;
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
            _ => return Err(ApiError::BadRequest("Unknown URI".to_string())),
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
        let rpc_params = params
            .to_rpc_params()
            .map_err(|e| ApiError::InternalServerError(e.to_string()))?;
        let body = Body::from(
            serde_json::to_string(&RequestSer::owned(Id::Number(0), method, rpc_params))
                .map_err(|e| ApiError::InternalServerError(e.to_string()))?,
        );

        *req.body_mut() = body;
    }

    Ok(())
}

/// Build Http Response
fn error_response(code: i32, msg: String) -> Response<Body> {
    let error = ErrorResponse::owned(ErrorObject::owned(code, msg, None::<()>), Id::Null);
    let response = serde_json::to_string(&error).expect("Unable to serialize");
    hyper::Response::builder()
        .header(
            "content-type",
            HeaderValue::from_static("application/json; charset=utf-8"),
        )
        .status(StatusCode::OK)
        .body(response.into())
        // Parsing `StatusCode` and `HeaderValue` is infalliable but
        // parsing body content is not.
        .expect("Unable to parse response body for type conversion")
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
        let builder = hyper::Response::builder().header(
            "content-type",
            HeaderValue::from_static("application/json; charset=utf-8"),
        );

        let result = match transform_request(&mut req) {
            Err(e) => Err(e),
            Ok(()) => {
                // Call the inner service and get a future that resolves to the response.
                let fut = self.inner.call(req);

                // Adjust the response if needed.
                let res_fut = async move {
                    let res = fut.await.map_err(|err| err.into())?;

                    let body = res.into_body();
                    let bytes = hyper::body::to_bytes(body).await?;

                    #[derive(serde::Deserialize, Debug)]
                    struct RpcPayload<'a> {
                        #[serde(borrow)]
                        result: &'a serde_json::value::RawValue,
                    }

                    let response: Response<Body> =
                        match serde_json::from_slice::<RpcPayload>(&bytes) {
                            Ok(payload) => builder
                                .status(StatusCode::OK)
                                .body(Body::from(payload.result.to_string()))
                                .expect("Unable to parse response body for type conversion"),
                            Err(e) => error_response(-32001, e.to_string()),
                        };

                    Ok(response)
                };

                Ok(res_fut)
            }
        };

        match result {
            Ok(fut) => Box::pin(fut),
            Err(e) => {
                let response = error_response(e.get_code(), e.to_string());
                let fut = async { Ok(response) };
                Box::pin(fut)
            }
        }
    }
}
