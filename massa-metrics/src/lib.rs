use crossbeam::channel::{bounded, unbounded, Receiver, RecvError, SendError, Sender};
use prometheus::{Counter, Gauge};

pub mod server;

#[derive(Clone)]
pub struct ChannelMetrics<T> {
    pub channel: (Sender<T>, Receiver<T>),
    name: String,
    actual_len: Gauge,
    received: Counter,
}

impl<T> ChannelMetrics<T> {
    /// Create new channel with optional capacity.
    fn new(name: String, capacity: Option<usize>) -> Self {
        let (s, r) = if let Some(capacity) = capacity {
            bounded::<T>(capacity)
        } else {
            unbounded::<T>()
        };
        let actual_len = Gauge::new(
            format!("{}_actual_size", name.clone()),
            "Actual length of channel",
        )
        .expect("Failed to create gauge");
        let received = Counter::new(
            format!("{}_total_receive", name.clone()),
            "Total received messages",
        )
        .expect("Failed to create counter");
        // TODO unwrap
        prometheus::register(Box::new(actual_len.clone())).unwrap();
        prometheus::register(Box::new(received.clone())).unwrap();
        Self {
            channel: (s, r),
            actual_len,
            received,
            name,
        }
    }

    fn send(&self, msg: T) -> Result<(), SendError<T>> {
        let res = self.channel.0.send(msg);
        self.actual_len.inc();
        res
    }

    fn len(&self) -> usize {
        self.channel.0.len()
    }

    fn recv(&self) -> Result<T, RecvError> {
        self.actual_len.dec();
        self.received.inc();
        self.channel.1.recv()
    }
}

mod test {
    use crate::server::bind_metrics;

    #[tokio::test]
    async fn test_channel_metrics() {
        // Runtime::new().unwrap().block_on(async {
        let addr = ([192, 168, 1, 183], 9898).into();
        bind_metrics(addr);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let channel = crate::ChannelMetrics::new("operations".to_string(), None);
        let channel2 = crate::ChannelMetrics::new("second_channel".to_string(), None);

        std::thread::sleep(std::time::Duration::from_secs(3));
        for i in 0..100 {
            channel.send(i).unwrap();
        }
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            channel.recv().unwrap();
        });

        channel2.send("Hello world".to_string()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1000));
    }

    #[test]
    fn test_channel_size() {
        let channel = crate::ChannelMetrics::new("test_size".to_string(), None);

        channel.send(1).unwrap();
        channel.send(2).unwrap();
        assert_eq!(channel.len(), 2);
    }
}
