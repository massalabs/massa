// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Unsigned WebSocket management
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(bound_map)]

use serde::Serialize;
use tokio_stream::wrappers::BroadcastStream;

pub use jsonrpsee::{core::error::SubscriptionClosed, SubscriptionSink};
pub use tokio::sync::broadcast::{self, Sender};

/// Brodcast the stream(sender) content via a WebSocket
pub fn broadcast_via_ws<T: Serialize + Send + Clone + 'static>(
    sender: Sender<T>,
    mut sink: SubscriptionSink,
) {
    let rx = BroadcastStream::new(sender.subscribe());
    tokio::spawn(async move {
        match sink.pipe_from_try_stream(rx).await {
            SubscriptionClosed::Success => {
                sink.close(SubscriptionClosed::Success);
            }
            SubscriptionClosed::RemotePeerAborted => (),
            SubscriptionClosed::Failed(err) => {
                sink.close(err);
            }
        };
    });
}
