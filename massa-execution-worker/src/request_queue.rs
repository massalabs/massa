// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a generic finite-size execution request queue with a oneshot result sender.

use massa_execution_exports::ExecutionError;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// Sender of the result of a request
pub(crate) type ResponseSender<R> = oneshot::Sender<Result<R, ExecutionError>>;

/// Represents an execution request T coupled with a oneshot sender for a result of type R
#[derive(Debug)]
pub(crate) struct RequestWithResponseSender<T, R> {
    /// The underlying execution request
    request: T,
    /// A oneshot sender to later send the execution output R (or an error).
    /// The receiving side can be awaited asynchronously or waited on synchronously.
    response_tx: ResponseSender<R>,
    /// When the request was created, i.e. queued by the caller
    enqueued_at: Instant,
}

impl<T, R> RequestWithResponseSender<T, R> {
    /// Create a new request with response sender, enqueued now
    ///
    /// # Arguments
    /// * `request`: the underlying request of type T
    /// * `response_tx`: a oneshot sender to later send the execution output R (or an error)
    pub fn new(request: T, response_tx: ResponseSender<R>) -> Self {
        RequestWithResponseSender {
            request,
            response_tx,
            enqueued_at: Instant::now(),
        }
    }

    /// Cancel the request by consuming the object and sending an error through the response channel.
    ///
    /// # Arguments
    /// * err: the error to send through the response channel
    pub fn cancel(self, err: ExecutionError) {
        // Send a message to the request's sender to signal the cancellation.
        // Ignore errors because they just mean that the emitter of the request
        // has dropped the receiver and does not need the response anymore.
        let _ = self.response_tx.send(Err(err));
    }

    /// Whether the caller gave up on the request (dropped the receiving side),
    /// in which case there is no point executing it.
    pub fn is_abandoned(&self) -> bool {
        self.response_tx.is_closed()
    }

    /// How long the request has been waiting since it was queued
    pub fn waited(&self) -> Duration {
        self.enqueued_at.elapsed()
    }

    /// Destructure self into a (request, response sender) pair
    pub fn into_request_sender_pair(self) -> (T, ResponseSender<R>) {
        (self.request, self.response_tx)
    }
}

/// Structure representing an execution request queue with maximal length.
/// Each request is a `RequestWithResponseSender` that comes with a oneshot sender
/// to return the execution result when the execution is over (or an error).
#[derive(Debug)]
pub(crate) struct RequestQueue<T, R> {
    /// Max number of item in the queue.
    /// When the queue is full, extra new items are cancelled and dropped.
    max_items: usize,

    /// The actual underlying queue
    queue: VecDeque<RequestWithResponseSender<T, R>>,
}

impl<T, R> RequestQueue<T, R> {
    /// Create a new request queue
    ///
    /// # Arguments
    /// * `max_items`: the maximal number of items in the queue. When full, extra new elements are cancelled and dropped.
    pub fn new(max_items: usize) -> Self {
        RequestQueue {
            max_items,
            queue: VecDeque::with_capacity(max_items),
        }
    }

    /// Returns the max number of items the queue can contain
    pub fn capacity(&self) -> usize {
        self.max_items
    }

    /// Extends Self with the contents of another `RequestQueue`.
    /// The contents of the incoming queue are appended last.
    /// Excess items with respect to `self.max_items` are canceled and dropped.
    pub fn extend(&mut self, mut other: RequestQueue<T, R>) {
        // compute the number of available item slots
        let free_slots = self.max_items.saturating_sub(self.queue.len());

        // if there are not enough available slots to fit the entire incoming queue
        if free_slots < other.queue.len() {
            // Truncate the incoming queue to the size that fits, cancelling excess items.
            // When the destination is already full this cancels the whole incoming queue:
            // `RequestQueue` does not cancel on drop, so dropping it instead would close
            // the response senders silently and already-accepted requests would fail with
            // a generic channel error instead of this clean capacity error.
            other.queue.drain(free_slots..).for_each(|req| {
                req.cancel(ExecutionError::ChannelError(
                    "maximal request queue capacity reached".into(),
                ))
            });
        }

        // append the kept part of the incoming queue
        self.queue.extend(other.queue);
    }

    /// Cancel all queued items.
    ///
    /// # Arguments
    /// * err: the error to send through the response channel of cancelled items
    pub fn cancel(&mut self, err: ExecutionError) {
        for req in self.queue.drain(..) {
            req.cancel(err.clone());
        }
    }

    /// Removes the requests that must not be executed anymore:
    /// * the ones whose caller gave up (dropped the receiving side) are dropped;
    /// * the ones that waited longer than `max_wait` are cancelled with a timeout error.
    pub fn remove_expired(&mut self, max_wait: Duration) {
        let mut kept = VecDeque::with_capacity(self.queue.len());
        for req in self.queue.drain(..) {
            if req.is_abandoned() {
                continue;
            }
            let waited = req.waited();
            if waited > max_wait {
                req.cancel(ExecutionError::ChannelError(format!(
                    "readonly request timed out after waiting {} ms in the queue (limit: {} ms)",
                    waited.as_millis(),
                    max_wait.as_millis()
                )));
                continue;
            }
            kept.push_back(req);
        }
        self.queue = kept;
    }

    /// Pop out the oldest element of the queue
    ///
    /// # Returns
    /// The oldest element of the queue, or None if the queue is empty
    pub fn pop(&mut self) -> Option<RequestWithResponseSender<T, R>> {
        self.queue.pop_front()
    }

    /// Push a new element at the end of the queue.
    /// May fail if maximum capacity is reached,
    /// in which case the request is canceled and dropped.
    pub fn push(&mut self, req: RequestWithResponseSender<T, R>) {
        // If the queue is already full, cancel the incoming request and return.
        if self.queue.len() >= self.max_items {
            req.cancel(ExecutionError::ChannelError(
                "maximal request queue capacity reached".into(),
            ));
            return;
        }

        // Append the incoming request to the end of the queue.
        self.queue.push_back(req);
    }

    /// Checks whether the queue is full
    ///
    /// # Returns
    /// true if the queue is full, false otherwise
    pub fn is_full(&self) -> bool {
        self.queue.len() >= self.max_items
    }

    /// Checks whether the queue is empty
    ///
    /// # Returns
    /// true if the queue is empty, false otherwise
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type ResponseReceiver = oneshot::Receiver<Result<u32, ExecutionError>>;

    fn make_request(request: u32) -> (RequestWithResponseSender<u32, u32>, ResponseReceiver) {
        let (response_tx, response_rx) = oneshot::channel();
        (
            RequestWithResponseSender::new(request, response_tx),
            response_rx,
        )
    }

    /// Assert that the request behind `response_rx` was cancelled with an error
    /// containing `expected`, not silently dropped.
    fn assert_cancelled_with(mut response_rx: ResponseReceiver, expected: &str) {
        match response_rx.try_recv() {
            Ok(Err(ExecutionError::ChannelError(msg))) => {
                assert!(msg.contains(expected), "unexpected error: {msg}")
            }
            other => panic!("expected a `{expected}` cancellation, got {:?}", other),
        }
    }

    fn assert_cancelled_for_capacity(response_rx: ResponseReceiver) {
        assert_cancelled_with(response_rx, "maximal request queue capacity reached");
    }

    #[test]
    fn test_extend_cancels_incoming_when_destination_is_full() {
        let mut destination = RequestQueue::<u32, u32>::new(1);
        let (req, _rx_kept) = make_request(0);
        destination.push(req);

        let mut incoming = RequestQueue::<u32, u32>::new(2);
        let (req1, rx1) = make_request(1);
        let (req2, rx2) = make_request(2);
        incoming.push(req1);
        incoming.push(req2);

        destination.extend(incoming);

        assert_cancelled_for_capacity(rx1);
        assert_cancelled_for_capacity(rx2);
        assert!(destination.is_full());
    }

    #[test]
    fn test_extend_cancels_only_the_excess_when_partially_full() {
        let mut destination = RequestQueue::<u32, u32>::new(2);
        let (req, _rx_kept) = make_request(0);
        destination.push(req);

        let mut incoming = RequestQueue::<u32, u32>::new(2);
        let (req1, mut rx1) = make_request(1);
        let (req2, rx2) = make_request(2);
        incoming.push(req1);
        incoming.push(req2);

        destination.extend(incoming);

        // the first incoming request fits: nothing received, sender still alive
        assert!(matches!(
            rx1.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        // the second one is in excess and must be cancelled with a clean error
        assert_cancelled_for_capacity(rx2);
        assert!(destination.is_full());
    }

    /// MASSA-011: a request that waited longer than the limit is answered with a
    /// timeout error, and one whose caller gave up is dropped without an answer.
    #[test]
    fn test_remove_expired() {
        let mut queue = RequestQueue::<u32, u32>::new(4);
        let (old, old_rx) = make_request(0);
        let (abandoned, abandoned_rx) = make_request(1);
        drop(abandoned_rx);
        queue.push(old);
        queue.push(abandoned);
        std::thread::sleep(Duration::from_millis(50));
        let (fresh, mut fresh_rx) = make_request(2);
        queue.push(fresh);

        queue.remove_expired(Duration::from_millis(25));

        assert_cancelled_with(old_rx, "readonly request timed out");
        // only the fresh request is left, still waiting for its answer
        let (request, _fresh_tx) = queue
            .pop()
            .expect("fresh request kept")
            .into_request_sender_pair();
        assert_eq!(request, 2);
        assert!(queue.pop().is_none());
        assert!(matches!(
            fresh_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }
}
