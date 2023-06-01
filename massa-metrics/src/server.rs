use std::net::SocketAddr;

use hyper::{
    header::CONTENT_TYPE,
    service::{make_service_fn, service_fn},
    Body, Request, Response,
};
use prometheus::{Encoder, TextEncoder};
use tokio::runtime::Runtime;

pub fn bind_metrics(addr: SocketAddr) {
    std::thread::spawn(move || {
        let runtime = Runtime::new().unwrap();
        runtime.block_on(async {
            let server = hyper::Server::bind(&addr).serve(make_service_fn(|_| async {
                Ok::<_, hyper::Error>(service_fn(serve_req))
            }));
            println!("Listening on http://{}", addr);
            server.await.unwrap();
        });
    });
}

async fn serve_req(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    if req.uri().path() != "/metrics" {
        // return hyper error
        return Ok(Response::builder()
            .status(404)
            .body(Body::from("Not Found"))
            .unwrap());
    }

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
