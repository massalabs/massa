use lazy_static::lazy_static;
use prometheus::{register_int_counter, Counter, Gauge, IntCounter};

pub mod channels;
mod server;

lazy_static! {
    static ref BLOCKS_COUNTER: IntCounter =
        register_int_counter!("blocks_counter", "blocks len").unwrap();

        static ref OPERATIONS_COUNTER: IntCounter =
        register_int_counter!("operations_counter", "operations counter").unwrap();
    // static ref A_INT_GAUGE: IntGauge = register_int_gauge!("A_int_gauge", "foobar").unwrap();
}

pub fn start_metrics_server(addr: std::net::SocketAddr) {
    server::bind_metrics(addr);
}

pub fn inc_blocks_counter() {
    BLOCKS_COUNTER.inc();
}

pub fn inc_operations_counter() {
    OPERATIONS_COUNTER.inc();
}

mod test {
    use crate::{channels::ChannelMetrics, start_metrics_server};

    #[tokio::test]
    async fn test_channel_metrics() {
        let addr = ([192, 168, 1, 183], 9898).into();

        start_metrics_server(addr);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let channel_metrics = ChannelMetrics::new("operations".to_string(), None);
        let channel2 = ChannelMetrics::new("second_channel".to_string(), None);

        std::thread::sleep(std::time::Duration::from_secs(3));
        for i in 0..100 {
            channel_metrics.send(i).unwrap();
        }
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            channel_metrics.recv().unwrap();
        });

        channel2.send("Hello world".to_string()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(100));
    }

    // #[test]
    // fn test_channel_size() {
    //     let channel = ChannelMetrics::new("test_size".to_string(), None);

    //     channel.send(1).unwrap();
    //     channel.send(2).unwrap();
    //     assert_eq!(channel.len(), 2);
    // }
}
