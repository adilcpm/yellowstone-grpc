use {
    bytes::Bytes,
    futures::{
        channel::mpsc,
        sink::{Sink, SinkExt},
        stream::Stream,
    },
    std::time::Duration,
    tonic::{
        codec::{CompressionEncoding, Streaming},
        metadata::{errors::InvalidMetadataValue, AsciiMetadataValue},
        service::interceptor::InterceptedService,
        transport::channel::{Channel, ClientTlsConfig, Endpoint},
        Request, Response, Status,
    },
    tonic_health::pb::{health_client::HealthClient, HealthCheckRequest, HealthCheckResponse},
    yellowstone_grpc_proto::prelude::{
        geyser_client::GeyserClient, CommitmentLevel, GetBlockHeightRequest,
        GetBlockHeightResponse, GetLatestBlockhashRequest, GetLatestBlockhashResponse,
        GetSlotRequest, GetSlotResponse, GetVersionRequest, GetVersionResponse,
        IsBlockhashValidRequest, IsBlockhashValidResponse, PingRequest, PongResponse,
        SubscribeRequest, SubscribeUpdate,
    },
};

pub use tonic::service::Interceptor;

#[derive(Debug, Clone)]
pub struct InterceptorXToken {
    pub x_token: Option<AsciiMetadataValue>,
}

impl Interceptor for InterceptorXToken {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if let Some(x_token) = self.x_token.clone() {
            request.metadata_mut().insert("x-token", x_token);
        }
        Ok(request)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GeyserGrpcClientError {
    #[error("gRPC status: {0}")]
    TonicStatus(#[from] Status),
    #[error("Failed to send subscribe request: {0}")]
    SubscribeSendError(#[from] mpsc::SendError),
}

pub type GeyserGrpcClientResult<T> = Result<T, GeyserGrpcClientError>;

pub struct GeyserGrpcClient<F> {
    pub health: HealthClient<InterceptedService<Channel, F>>,
    pub geyser: GeyserClient<InterceptedService<Channel, F>>,
}

impl GeyserGrpcClient<()> {
    pub fn build_from_shared(
        endpoint: impl Into<Bytes>,
    ) -> GeyserGrpcBuilderResult<GeyserGrpcBuilder> {
        Ok(GeyserGrpcBuilder::new(Endpoint::from_shared(endpoint)?))
    }

    pub fn build_from_static(endpoint: &'static str) -> GeyserGrpcBuilder {
        GeyserGrpcBuilder::new(Endpoint::from_static(endpoint))
    }
}

impl<F: Interceptor> GeyserGrpcClient<F> {
    pub fn new(
        health: HealthClient<InterceptedService<Channel, F>>,
        geyser: GeyserClient<InterceptedService<Channel, F>>,
    ) -> Self {
        Self { health, geyser }
    }

    // Health
    pub async fn health_check(&mut self) -> GeyserGrpcClientResult<HealthCheckResponse> {
        let request = HealthCheckRequest {
            service: "geyser.Geyser".to_owned(),
        };
        let response = self.health.check(request).await?;
        Ok(response.into_inner())
    }

    pub async fn health_watch(
        &mut self,
    ) -> GeyserGrpcClientResult<impl Stream<Item = Result<HealthCheckResponse, Status>>> {
        let request = HealthCheckRequest {
            service: "geyser.Geyser".to_owned(),
        };
        let response = self.health.watch(request).await?;
        Ok(response.into_inner())
    }

    // Subscribe
    pub async fn subscribe(
        &mut self,
    ) -> GeyserGrpcClientResult<(
        impl Sink<SubscribeRequest, Error = mpsc::SendError>,
        impl Stream<Item = Result<SubscribeUpdate, Status>>,
    )> {
        self.subscribe_with_request(None).await
    }

    pub async fn subscribe_with_request(
        &mut self,
        request: Option<SubscribeRequest>,
    ) -> GeyserGrpcClientResult<(
        impl Sink<SubscribeRequest, Error = mpsc::SendError>,
        impl Stream<Item = Result<SubscribeUpdate, Status>>,
    )> {
        let (mut subscribe_tx, subscribe_rx) = mpsc::unbounded();
        if let Some(request) = request {
            subscribe_tx
                .send(request)
                .await
                .map_err(GeyserGrpcClientError::SubscribeSendError)?;
        }
        let response: Response<Streaming<SubscribeUpdate>> =
            self.geyser.subscribe(subscribe_rx).await?;
        Ok((subscribe_tx, response.into_inner()))
    }

    pub async fn subscribe_once(
        &mut self,
        request: SubscribeRequest,
    ) -> GeyserGrpcClientResult<impl Stream<Item = Result<SubscribeUpdate, Status>>> {
        self.subscribe_with_request(Some(request))
            .await
            .map(|(_sink, stream)| stream)
    }

    // RPC calls
    pub async fn ping(&mut self, count: i32) -> GeyserGrpcClientResult<PongResponse> {
        let message = PingRequest { count };
        let request = tonic::Request::new(message);
        let response = self.geyser.ping(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_latest_blockhash(
        &mut self,
        commitment: Option<CommitmentLevel>,
    ) -> GeyserGrpcClientResult<GetLatestBlockhashResponse> {
        let request = tonic::Request::new(GetLatestBlockhashRequest {
            commitment: commitment.map(|value| value as i32),
        });
        let response = self.geyser.get_latest_blockhash(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_block_height(
        &mut self,
        commitment: Option<CommitmentLevel>,
    ) -> GeyserGrpcClientResult<GetBlockHeightResponse> {
        let request = tonic::Request::new(GetBlockHeightRequest {
            commitment: commitment.map(|value| value as i32),
        });
        let response = self.geyser.get_block_height(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_slot(
        &mut self,
        commitment: Option<CommitmentLevel>,
    ) -> GeyserGrpcClientResult<GetSlotResponse> {
        let request = tonic::Request::new(GetSlotRequest {
            commitment: commitment.map(|value| value as i32),
        });
        let response = self.geyser.get_slot(request).await?;
        Ok(response.into_inner())
    }

    pub async fn is_blockhash_valid(
        &mut self,
        blockhash: String,
        commitment: Option<CommitmentLevel>,
    ) -> GeyserGrpcClientResult<IsBlockhashValidResponse> {
        let request = tonic::Request::new(IsBlockhashValidRequest {
            blockhash,
            commitment: commitment.map(|value| value as i32),
        });
        let response = self.geyser.is_blockhash_valid(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_version(&mut self) -> GeyserGrpcClientResult<GetVersionResponse> {
        let request = tonic::Request::new(GetVersionRequest {});
        let response = self.geyser.get_version(request).await?;
        Ok(response.into_inner())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GeyserGrpcBuilderError {
    #[error("Failed to parse x-token: {0}")]
    MetadataValueError(#[from] InvalidMetadataValue),
    #[error("Invalid X-Token length: {0}, expected 28")]
    InvalidXTokenLength(usize),
    #[error("gRPC transport error: {0}")]
    TonicError(#[from] tonic::transport::Error),
    #[error("tonic::transport::Channel should be created, use `connect` or `connect_lazy` first")]
    EmptyChannel,
}

pub type GeyserGrpcBuilderResult<T> = Result<T, GeyserGrpcBuilderError>;

#[derive(Debug)]
pub struct GeyserGrpcBuilder {
    pub endpoint: Endpoint,
    pub channel: Option<Channel>,

    pub x_token: Option<AsciiMetadataValue>,

    pub send_compressed: Option<CompressionEncoding>,
    pub accept_compressed: Option<CompressionEncoding>,
    pub max_decoding_message_size: Option<usize>,
    pub max_encoding_message_size: Option<usize>,
}

impl GeyserGrpcBuilder {
    // Create new builder
    fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            channel: None,

            x_token: None,

            send_compressed: None,
            accept_compressed: None,
            max_decoding_message_size: None,
            max_encoding_message_size: None,
        }
    }

    pub fn from_shared(endpoint: impl Into<Bytes>) -> GeyserGrpcBuilderResult<Self> {
        Ok(Self::new(Endpoint::from_shared(endpoint)?))
    }

    pub fn from_static(endpoint: &'static str) -> Self {
        Self::new(Endpoint::from_static(endpoint))
    }

    // Create client
    pub fn build(mut self) -> GeyserGrpcBuilderResult<GeyserGrpcClient<impl Interceptor>> {
        let channel = self
            .channel
            .take()
            .ok_or(GeyserGrpcBuilderError::EmptyChannel)?;
        let interceptor = InterceptorXToken {
            x_token: self.x_token,
        };

        let mut geyser = GeyserClient::with_interceptor(channel.clone(), interceptor.clone());
        if let Some(encoding) = self.send_compressed {
            geyser = geyser.send_compressed(encoding);
        }
        if let Some(encoding) = self.accept_compressed {
            geyser = geyser.accept_compressed(encoding);
        }
        if let Some(limit) = self.max_decoding_message_size {
            geyser = geyser.max_decoding_message_size(limit);
        }
        if let Some(limit) = self.max_encoding_message_size {
            geyser = geyser.max_encoding_message_size(limit);
        }

        Ok(GeyserGrpcClient::new(
            HealthClient::with_interceptor(channel, interceptor),
            geyser,
        ))
    }

    // Set x-token
    pub fn x_token<T>(self, x_token: Option<T>) -> GeyserGrpcBuilderResult<Self>
    where
        T: TryInto<AsciiMetadataValue, Error = InvalidMetadataValue>,
    {
        Ok(Self {
            x_token: match x_token {
                Some(x_token) => {
                    let x_token = x_token.try_into()?;
                    if x_token.is_empty() {
                        return Err(GeyserGrpcBuilderError::InvalidXTokenLength(x_token.len()));
                    }
                    Some(x_token)
                }
                None => None,
            },
            ..self
        })
    }

    // Endpoint options
    pub async fn connect(self) -> GeyserGrpcBuilderResult<Self> {
        Ok(Self {
            channel: Some(self.endpoint.connect().await?),
            ..self
        })
    }

    pub fn connect_lazy(self) -> Self {
        Self {
            channel: Some(self.endpoint.connect_lazy()),
            ..self
        }
    }

    pub fn connect_timeout(self, dur: Duration) -> Self {
        Self {
            endpoint: self.endpoint.connect_timeout(dur),
            ..self
        }
    }

    pub fn timeout(self, dur: Duration) -> Self {
        Self {
            endpoint: self.endpoint.timeout(dur),
            ..self
        }
    }

    pub fn tls_config(self, tls_config: ClientTlsConfig) -> GeyserGrpcBuilderResult<Self> {
        Ok(Self {
            endpoint: self.endpoint.tls_config(tls_config)?,
            ..self
        })
    }

    pub fn buffer_size(self, sz: impl Into<Option<usize>>) -> Self {
        Self {
            endpoint: self.endpoint.buffer_size(sz),
            ..self
        }
    }

    pub fn http2_adaptive_window(self, enabled: bool) -> Self {
        Self {
            endpoint: self.endpoint.http2_adaptive_window(enabled),
            ..self
        }
    }

    pub fn http2_keep_alive_interval(self, interval: Duration) -> Self {
        Self {
            endpoint: self.endpoint.http2_keep_alive_interval(interval),
            ..self
        }
    }

    pub fn initial_connection_window_size(self, sz: impl Into<Option<u32>>) -> Self {
        Self {
            endpoint: self.endpoint.initial_connection_window_size(sz),
            ..self
        }
    }

    pub fn initial_stream_window_size(self, sz: impl Into<Option<u32>>) -> Self {
        Self {
            endpoint: self.endpoint.initial_stream_window_size(sz),
            ..self
        }
    }

    pub fn keep_alive_timeout(self, duration: Duration) -> Self {
        Self {
            endpoint: self.endpoint.keep_alive_timeout(duration),
            ..self
        }
    }

    pub fn keep_alive_while_idle(self, enabled: bool) -> Self {
        Self {
            endpoint: self.endpoint.keep_alive_while_idle(enabled),
            ..self
        }
    }

    pub fn tcp_keepalive(self, tcp_keepalive: Option<Duration>) -> Self {
        Self {
            endpoint: self.endpoint.tcp_keepalive(tcp_keepalive),
            ..self
        }
    }

    pub fn tcp_nodelay(self, enabled: bool) -> Self {
        Self {
            endpoint: self.endpoint.tcp_nodelay(enabled),
            ..self
        }
    }

    // Geyser options
    pub fn send_compressed(self, encoding: CompressionEncoding) -> Self {
        Self {
            send_compressed: Some(encoding),
            ..self
        }
    }

    pub fn accept_compressed(self, encoding: CompressionEncoding) -> Self {
        Self {
            accept_compressed: Some(encoding),
            ..self
        }
    }

    pub fn max_decoding_message_size(self, limit: usize) -> Self {
        Self {
            max_decoding_message_size: Some(limit),
            ..self
        }
    }

    pub fn max_encoding_message_size(self, limit: usize) -> Self {
        Self {
            max_encoding_message_size: Some(limit),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GeyserGrpcBuilderError, GeyserGrpcClient};

    #[tokio::test]
    async fn test_channel_https_success() {
        let endpoint = "https://ams17.rpcpool.com:443";
        let x_token = "1000000000000000000000000007";

        let res = GeyserGrpcClient::build_from_shared(endpoint);
        assert!(res.is_ok());

        let res = res.unwrap().x_token(Some(x_token));
        assert!(res.is_ok());

        let res = res.unwrap().connect_lazy().build();
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_channel_http_success() {
        let endpoint = "http://127.0.0.1:10000";
        let x_token = "1234567891012141618202224268";

        let res = GeyserGrpcClient::build_from_shared(endpoint);
        assert!(res.is_ok());

        let res = res.unwrap().x_token(Some(x_token));
        assert!(res.is_ok());

        let res = res.unwrap().connect_lazy().build();
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_channel_invalid_token_some() {
        let endpoint = "http://127.0.0.1:10000";
        let x_token = "";

        let res = GeyserGrpcClient::build_from_shared(endpoint);
        assert!(res.is_ok());

        let res = res.unwrap().x_token(Some(x_token));
        assert!(matches!(
            res,
            Err(GeyserGrpcBuilderError::InvalidXTokenLength(_))
        ));
    }

    #[tokio::test]
    async fn test_channel_invalid_token_none() {
        let endpoint = "http://127.0.0.1:10000";

        let res = GeyserGrpcClient::build_from_shared(endpoint);
        assert!(res.is_ok());

        let res = res.unwrap().x_token::<String>(None);
        assert!(res.is_ok());

        let res = res.unwrap().connect_lazy().build();
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_channel_invalid_uri() {
        let endpoint = "sites/files/images/picture.png";

        let res = GeyserGrpcClient::build_from_shared(endpoint);
        assert_eq!(
            format!("{:?}", res),
            "Err(TonicError(tonic::transport::Error(InvalidUri, InvalidUri(InvalidFormat))))"
                .to_owned()
        );
    }
}
