// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Unsigned WebSocket management
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(bound_map)]

use jsonrpsee::{core::error::SubscriptionClosed, SubscriptionSink};
use serde::Serialize;
use tokio::sync::broadcast::Sender;
use tokio_stream::wrappers::BroadcastStream;

/// Re-exports libraries to not require any additional
/// dependencies to be explicitly added on the module(s) side.
#[doc(hidden)]
pub mod __reexports {
    pub use jsonrpsee;
    pub use tokio;
}

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
