// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a generic finite-size execution request queue with an MPSC-based result sender.

use massa_channel::sender::MassaSender;
use massa_execution_exports::ExecutionError;
use std::collections::VecDeque;

/// Represents an execution request T coupled with an MPSC sender for a result of type R
#[derive(Debug)]
pub(crate) struct RequestWithResponseSender<T, R> {
    /// The underlying execution request
    request: T,
    /// An std::mpsc::Sender to later send the execution output R (or an error)
    response_tx: MassaSender<Result<R, ExecutionError>>,
}

impl<T, R> RequestWithResponseSender<T, R> {
    /// Create a new request with response sender
    ///
    /// # Arguments
    /// * `request`: the underlying request of type T
    /// * `response_tx`: an `std::mpsc::Sender` to later send the execution output R (or an error)
    pub fn new(request: T, response_tx: MassaSender<Result<R, ExecutionError>>) -> Self {
        RequestWithResponseSender {
            request,
            response_tx,
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

    /// Destructure self into a (request, response sender) pair
    pub fn into_request_sender_pair(self) -> (T, MassaSender<Result<R, ExecutionError>>) {
        (self.request, self.response_tx)
    }
}

/// Structure representing an execution request queue with maximal length.
/// Each request is a `RequestWithResponseSender` that comes with an MPSC sender
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

        // If there are no available slots remaining, cancel the whole incoming queue.
        // `RequestQueue` does not cancel on drop, so returning without cancelling would
        // drop the response senders silently: already-accepted requests would fail with
        // a generic channel error instead of this clean capacity error.
        if free_slots == 0 {
            other.cancel(ExecutionError::ChannelError(
                "maximal request queue capacity reached".into(),
            ));
            return;
        }

        // if there are not enough available slots to fit the entire incoming queue
        if free_slots < other.queue.len() {
            // truncate the incoming queue to the size that fits, cancelling excess items
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
    ///
    /// # Returns
    /// The oldest element of the queue, or None if the queue is empty
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

    /// Take all the elements into a new queue and reset the current queue
    /*pub fn take(&mut self) -> Self {
        RequestQueue {
            max_items: self.max_items,
            queue: std::mem::take(&mut self.queue),
        }
    }*/

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
    use massa_channel::receiver::MassaReceiver;
    use massa_channel::MassaChannel;

    fn make_request(
        request: u32,
    ) -> (
        RequestWithResponseSender<u32, u32>,
        MassaReceiver<Result<u32, ExecutionError>>,
    ) {
        let (response_tx, response_rx) = MassaChannel::new("test_request_queue".to_string(), None);
        (
            RequestWithResponseSender::new(request, response_tx),
            response_rx,
        )
    }

    /// Assert that the request behind `response_rx` was cancelled with a clean
    /// capacity error, not silently dropped.
    fn assert_cancelled_for_capacity(response_rx: MassaReceiver<Result<u32, ExecutionError>>) {
        match response_rx.recv() {
            Ok(Err(ExecutionError::ChannelError(msg))) => {
                assert!(msg.contains("maximal request queue capacity reached"))
            }
            other => panic!("expected a capacity cancellation error, got {:?}", other),
        }
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
        let (req1, rx1) = make_request(1);
        let (req2, rx2) = make_request(2);
        incoming.push(req1);
        incoming.push(req2);

        destination.extend(incoming);

        // the first incoming request fits: nothing received, sender still alive
        assert!(rx1.try_recv().unwrap_err().is_empty());
        // the second one is in excess and must be cancelled with a clean error
        assert_cancelled_for_capacity(rx2);
        assert!(destination.is_full());
    }
}
