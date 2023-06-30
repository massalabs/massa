use std::net::SocketAddr;

use hyper::{
    header::CONTENT_TYPE,
    service::{make_service_fn, service_fn},
    Body, Request, Response,
};
use prometheus::{Encoder, TextEncoder};
use tracing::{error, info};

use crate::MetricsStopper;

#[allow(dead_code)]
pub(crate) fn bind_metrics(addr: SocketAddr) -> MetricsStopper {
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("error on build tokio runtime for metrics server");

        rt.block_on(async {
            let server = hyper::Server::bind(&addr).serve(make_service_fn(|_| async {
                Ok::<_, hyper::Error>(service_fn(serve_req))
            }));

            let graceful_server = server.with_graceful_shutdown(async {
                rx.await.ok();
            });
            info!("METRICS | listening on http://{}", addr);
            if let Err(e) = graceful_server.await {
                error!("metrics server error: {}", e);
            }
            info!("METRICS | server stopped");
        });
    });
    MetricsStopper {
        stopper: Some(tx),
        stop_handle: Some(handle),
    }
}

#[allow(dead_code)]
async fn serve_req(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    if req.uri().path() != "/metrics" {
        // return hyper error
        Ok(Response::builder()
            .status(404)
            .body(Body::from("Not Found"))
            .unwrap())
    } else {
        let encoder = TextEncoder::new();
        let mut buffer = vec![];
        encoder
            .encode(&prometheus::gather(), &mut buffer)
            .expect("Failed to encode metrics");

        let response = Response::builder()
            .status(200)
            .header(CONTENT_TYPE, encoder.format_type())
            .body(Body::from(buffer))
            .unwrap();

        Ok(response)
    }
}
