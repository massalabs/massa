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
    use crate::{channels::MassaChannel, start_metrics_server};

    #[tokio::test]
    async fn test_channel_metrics() {
        let addr = ([192, 168, 1, 183], 9898).into();

        start_metrics_server(addr);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let (sender, receiver) = MassaChannel::new("operations".to_string(), None);

        let (sender2, receiver2) = MassaChannel::new("second_channel".to_string(), None);

        sender2.send("hello_world".to_string()).unwrap();
        let data = receiver2.recv().unwrap();
        assert_eq!(data, "hello_world".to_string());

        for i in 0..100 {
            sender.send(i).unwrap();
        }

        for _i in 0..20 {
            receiver.recv().unwrap();
        }

        assert_eq!(receiver.len(), 80);
        std::thread::sleep(std::time::Duration::from_secs(5));
        drop(sender2);
        drop(receiver2);
        std::thread::sleep(std::time::Duration::from_secs(100));
    }

    #[tokio::test]
    async fn test_channel() {
        let addr = ([192, 168, 1, 183], 9898).into();

        start_metrics_server(addr);
        std::thread::sleep(std::time::Duration::from_millis(500));

        let (sender, receiver) = MassaChannel::new("test2".to_string(), None);

        let cloned = receiver.clone();

        sender.send("msg".to_string()).unwrap();

        std::thread::spawn(move || {
            dbg!("spawned");

            loop {
                dbg!("loop");
                dbg!(receiver.recv().unwrap());
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
        std::thread::sleep(std::time::Duration::from_secs(2));
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));

            drop(sender);
        });

        std::thread::sleep(std::time::Duration::from_secs(20));
    }
}
