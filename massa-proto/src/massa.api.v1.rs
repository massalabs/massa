/// commons
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Version {
    /// string field
    #[prost(string, tag = "1")]
    pub version: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureSharePayload {
    /// / The public key of the content creator
    #[prost(string, tag = "1")]
    pub creator_public_key: ::prost::alloc::string::String,
    /// / The signature of the content
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// / The serialized version of the content
    #[prost(bytes = "vec", tag = "3")]
    pub serialized_content: ::prost::alloc::vec::Vec<u8>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Slot {
    /// fixed64 field
    #[prost(uint64, tag = "1")]
    pub period: u64,
    /// float field
    #[prost(float, tag = "2")]
    pub thread: f32,
}
/// region Endorsement
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EndorsementInfo {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// bool field
    #[prost(bool, tag = "2")]
    pub in_pool: bool,
    /// string field
    #[prost(string, repeated, tag = "3")]
    pub in_blocks: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// bool field
    #[prost(bool, tag = "4")]
    pub is_final: bool,
    /// object field
    #[prost(message, optional, tag = "5")]
    pub endorsement: ::core::option::Option<Endorsement>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Endorsement {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub slot: ::core::option::Option<Slot>,
    /// string field
    #[prost(uint32, tag = "2")]
    pub index: u32,
    /// string field
    #[prost(string, tag = "3")]
    pub endorsed_block: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EndorsementContent {
    /// string field
    #[prost(string, tag = "1")]
    pub sender_public_key: ::prost::alloc::string::String,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub slot: ::core::option::Option<Slot>,
    /// float field
    #[prost(uint32, tag = "3")]
    pub index: u32,
    /// string field
    #[prost(string, tag = "4")]
    pub endorsed_block: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EndorsementId {
    /// string field
    #[prost(string, tag = "1")]
    pub value: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureShareEndorsement {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub content: ::core::option::Option<Endorsement>,
    /// string field
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "3")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "4")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "5")]
    pub id: ::prost::alloc::string::String,
}
/// GetVersionRequest holds request from GetVersion
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetVersionRequest {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub version: ::prost::alloc::string::String,
}
/// GetVersionResponse holds response from GetVersion
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetVersionResponse {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub version: ::prost::alloc::string::String,
}
/// SendBlocksRequest holds parameters to SendBlocks
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendBlocksRequest {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub block: ::core::option::Option<SecureSharePayload>,
}
/// SendBlocksResponse holds response from SendBlocks
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendBlocksResponse {
    /// string value
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(oneof = "send_blocks_response::Message", tags = "2, 3")]
    pub message: ::core::option::Option<send_blocks_response::Message>,
}
/// Nested message and enum types in `SendBlocksResponse`.
pub mod send_blocks_response {
    /// object field
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Message {
        #[prost(message, tag = "2")]
        Result(super::BlockResult),
        #[prost(message, tag = "3")]
        Error(super::super::super::super::google::rpc::Status),
    }
}
/// Holds Block response
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockResult {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
/// SendEndorsementsRequest holds parameters to SendEndorsements
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendEndorsementsRequest {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub endorsement: ::core::option::Option<Endorsement>,
}
/// SendEndorsementsResponse holds response from SendEndorsements
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendEndorsementsResponse {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(oneof = "send_endorsements_response::Message", tags = "2, 3")]
    pub message: ::core::option::Option<send_endorsements_response::Message>,
}
/// Nested message and enum types in `SendEndorsementsResponse`.
pub mod send_endorsements_response {
    /// object field
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Message {
        #[prost(message, tag = "2")]
        Result(super::EndorsementResult),
        #[prost(message, tag = "3")]
        Error(super::super::super::super::google::rpc::Status),
    }
}
/// Holds Endorsement response
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EndorsementResult {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
}
/// SendOperationsRequest holds parameters to SendOperations
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendOperationsRequest {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(message, repeated, tag = "2")]
    pub operations: ::prost::alloc::vec::Vec<SecureSharePayload>,
}
/// SendOperationsResponse holds response from SendOperations
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SendOperationsResponse {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(oneof = "send_operations_response::Message", tags = "2, 3")]
    pub message: ::core::option::Option<send_operations_response::Message>,
}
/// Nested message and enum types in `SendOperationsResponse`.
pub mod send_operations_response {
    /// object field
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Message {
        #[prost(message, tag = "2")]
        Result(super::OperationResult),
        #[prost(message, tag = "3")]
        Error(super::super::super::super::google::rpc::Status),
    }
}
/// Holds Operation response
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationResult {
    /// string field
    #[prost(string, repeated, tag = "1")]
    pub ids: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// Generated client implementations.
pub mod grpc_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /// Massa gRPC service
    #[derive(Debug, Clone)]
    pub struct GrpcClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl GrpcClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> GrpcClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> GrpcClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
            >>::Error: Into<StdError> + Send + Sync,
        {
            GrpcClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        /// GetVersion
        pub async fn get_version(
            &mut self,
            request: impl tonic::IntoRequest<super::GetVersionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetVersionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/GetVersion",
            );
            self.inner.unary(request.into_request(), path, codec).await
        }
        /// SendBlocks
        pub async fn send_blocks(
            &mut self,
            request: impl tonic::IntoStreamingRequest<Message = super::SendBlocksRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::SendBlocksResponse>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/SendBlocks",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        /// SendEndorsements
        pub async fn send_endorsements(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::SendEndorsementsRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::SendEndorsementsResponse>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/SendEndorsements",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        /// SendOperations
        pub async fn send_operations(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::SendOperationsRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::SendOperationsResponse>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/massa.api.v1.Grpc/SendOperations",
            );
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod grpc_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with GrpcServer.
    #[async_trait]
    pub trait Grpc: Send + Sync + 'static {
        /// GetVersion
        async fn get_version(
            &self,
            request: tonic::Request<super::GetVersionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetVersionResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the SendBlocks method.
        type SendBlocksStream: futures_core::Stream<
                Item = std::result::Result<super::SendBlocksResponse, tonic::Status>,
            >
            + Send
            + 'static;
        /// SendBlocks
        async fn send_blocks(
            &self,
            request: tonic::Request<tonic::Streaming<super::SendBlocksRequest>>,
        ) -> std::result::Result<tonic::Response<Self::SendBlocksStream>, tonic::Status>;
        /// Server streaming response type for the SendEndorsements method.
        type SendEndorsementsStream: futures_core::Stream<
                Item = std::result::Result<
                    super::SendEndorsementsResponse,
                    tonic::Status,
                >,
            >
            + Send
            + 'static;
        /// SendEndorsements
        async fn send_endorsements(
            &self,
            request: tonic::Request<tonic::Streaming<super::SendEndorsementsRequest>>,
        ) -> std::result::Result<
            tonic::Response<Self::SendEndorsementsStream>,
            tonic::Status,
        >;
        /// Server streaming response type for the SendOperations method.
        type SendOperationsStream: futures_core::Stream<
                Item = std::result::Result<super::SendOperationsResponse, tonic::Status>,
            >
            + Send
            + 'static;
        /// SendOperations
        async fn send_operations(
            &self,
            request: tonic::Request<tonic::Streaming<super::SendOperationsRequest>>,
        ) -> std::result::Result<
            tonic::Response<Self::SendOperationsStream>,
            tonic::Status,
        >;
    }
    /// Massa gRPC service
    #[derive(Debug)]
    pub struct GrpcServer<T: Grpc> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: Grpc> GrpcServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for GrpcServer<T>
    where
        T: Grpc,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/massa.api.v1.Grpc/GetVersion" => {
                    #[allow(non_camel_case_types)]
                    struct GetVersionSvc<T: Grpc>(pub Arc<T>);
                    impl<T: Grpc> tonic::server::UnaryService<super::GetVersionRequest>
                    for GetVersionSvc<T> {
                        type Response = super::GetVersionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetVersionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move { (*inner).get_version(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetVersionSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/SendBlocks" => {
                    #[allow(non_camel_case_types)]
                    struct SendBlocksSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<super::SendBlocksRequest>
                    for SendBlocksSvc<T> {
                        type Response = super::SendBlocksResponse;
                        type ResponseStream = T::SendBlocksStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::SendBlocksRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move { (*inner).send_blocks(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SendBlocksSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/SendEndorsements" => {
                    #[allow(non_camel_case_types)]
                    struct SendEndorsementsSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<super::SendEndorsementsRequest>
                    for SendEndorsementsSvc<T> {
                        type Response = super::SendEndorsementsResponse;
                        type ResponseStream = T::SendEndorsementsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::SendEndorsementsRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).send_endorsements(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SendEndorsementsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/massa.api.v1.Grpc/SendOperations" => {
                    #[allow(non_camel_case_types)]
                    struct SendOperationsSvc<T: Grpc>(pub Arc<T>);
                    impl<
                        T: Grpc,
                    > tonic::server::StreamingService<super::SendOperationsRequest>
                    for SendOperationsSvc<T> {
                        type Response = super::SendOperationsResponse;
                        type ResponseStream = T::SendOperationsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::SendOperationsRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).send_operations(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SendOperationsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        Ok(
                            http::Response::builder()
                                .status(200)
                                .header("grpc-status", "12")
                                .header("content-type", "application/grpc")
                                .body(empty_body())
                                .unwrap(),
                        )
                    })
                }
            }
        }
    }
    impl<T: Grpc> Clone for GrpcServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    impl<T: Grpc> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: Grpc> tonic::server::NamedService for GrpcServer<T> {
        const NAME: &'static str = "massa.api.v1.Grpc";
    }
}
/// region Operation
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationInfo {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// string field
    #[prost(string, repeated, tag = "2")]
    pub in_blocks: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// bool field
    #[prost(bool, tag = "3")]
    pub in_pool: bool,
    /// bool field
    #[prost(bool, tag = "4")]
    pub is_final: bool,
    /// object field
    #[prost(message, optional, tag = "5")]
    pub operation: ::core::option::Option<SecureShareOperation>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Roll {
    /// int64 field
    #[prost(int64, tag = "1")]
    pub roll_count: i64,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationId {
    /// string field
    #[prost(string, tag = "1")]
    pub value: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationType {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub transaction: ::core::option::Option<Transaction>,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub execut_sc: ::core::option::Option<ExecuteSc>,
    /// object field
    #[prost(message, optional, tag = "3")]
    pub call_sc: ::core::option::Option<CallSc>,
    /// object field
    #[prost(message, optional, tag = "4")]
    pub roll_buy: ::core::option::Option<RollBuy>,
    /// object field
    #[prost(message, optional, tag = "5")]
    pub roll_sell: ::core::option::Option<RollSell>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Transaction {
    /// string field
    #[prost(string, tag = "1")]
    pub amount: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub recipient_address: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CallSc {
    /// string field
    #[prost(string, tag = "1")]
    pub target_addr: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub target_func: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "3")]
    pub param: ::prost::alloc::string::String,
    /// float field
    #[prost(float, tag = "4")]
    pub max_gas: f32,
    /// float field
    #[prost(float, tag = "5")]
    pub sequential_coins: f32,
    /// float field
    #[prost(float, tag = "6")]
    pub parallel_coins: f32,
    /// float field
    #[prost(float, tag = "7")]
    pub gas_price: f32,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ExecuteSc {
    /// float field
    #[prost(float, repeated, tag = "1")]
    pub data: ::prost::alloc::vec::Vec<f32>,
    /// float field
    #[prost(float, tag = "2")]
    pub max_gas: f32,
    /// string field
    #[prost(string, tag = "3")]
    pub coins: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "4")]
    pub gas_price: ::prost::alloc::string::String,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RollBuy {
    /// float field
    #[prost(float, tag = "1")]
    pub roll_count: f32,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RollSell {
    /// float field
    #[prost(float, tag = "1")]
    pub roll_count: f32,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OperationInput {
    /// string field
    #[prost(string, tag = "1")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// string field
    #[prost(string, repeated, tag = "3")]
    pub serialized_content: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureShareOperation {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub content: ::core::option::Option<OperationType>,
    /// string field
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "3")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "4")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "5")]
    pub id: ::prost::alloc::string::String,
}
/// region Block
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Block {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub header: ::core::option::Option<SecureShareBlockHeader>,
    /// object field
    #[prost(string, repeated, tag = "2")]
    pub operations: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockInfo {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field field
    #[prost(message, optional, tag = "2")]
    pub content: ::core::option::Option<BlockInfoContent>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockInfoContent {
    /// bool field
    #[prost(bool, tag = "1")]
    pub is_final: bool,
    /// bool field
    #[prost(bool, tag = "2")]
    pub is_stale: bool,
    /// bool field
    #[prost(bool, tag = "3")]
    pub is_in_blockclique: bool,
    /// object field
    #[prost(message, optional, tag = "4")]
    pub block: ::core::option::Option<Block>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BlockHeader {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub slot: ::core::option::Option<Slot>,
    /// string field
    #[prost(string, repeated, tag = "2")]
    pub parents: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// string field
    #[prost(string, tag = "3")]
    pub operation_merkle_root: ::prost::alloc::string::String,
    /// object field
    #[prost(message, repeated, tag = "4")]
    pub endorsements: ::prost::alloc::vec::Vec<SecureShareEndorsement>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FilledBlock {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub header: ::core::option::Option<SecureShareBlockHeader>,
    /// object field
    #[prost(message, repeated, tag = "2")]
    pub operations: ::prost::alloc::vec::Vec<OperationInfo>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FilledBlockInfo {
    /// string field
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// object field
    #[prost(message, optional, tag = "2")]
    pub content: ::core::option::Option<FilledBlockInfoContent>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FilledBlockInfoContent {
    /// bool field
    #[prost(bool, tag = "1")]
    pub is_final: bool,
    /// bool field
    #[prost(bool, tag = "2")]
    pub is_stale: bool,
    /// bool field
    #[prost(bool, tag = "3")]
    pub is_in_blockclique: bool,
    /// object field
    #[prost(message, optional, tag = "4")]
    pub block: ::core::option::Option<FilledBlock>,
}
/// message struct
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SecureShareBlockHeader {
    /// object field
    #[prost(message, optional, tag = "1")]
    pub content: ::core::option::Option<BlockHeader>,
    /// string field
    #[prost(string, tag = "2")]
    pub signature: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "3")]
    pub content_creator_pub_key: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "4")]
    pub content_creator_address: ::prost::alloc::string::String,
    /// string field
    #[prost(string, tag = "5")]
    pub id: ::prost::alloc::string::String,
}
