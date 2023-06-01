use crossbeam::channel::{bounded, unbounded, Receiver, RecvError, SendError, Sender};
use prometheus::{Counter, Gauge};

pub mod channels;
mod server;

pub fn start_metrics_server(addr: std::net::SocketAddr) {
    server::bind_metrics(addr);
}

mod test {
    use crate::{channels::ChannelMetrics, server::bind_metrics};

    #[tokio::test]
    async fn test_channel_metrics() {
        // Runtime::new().unwrap().block_on(async {
        let addr = ([192, 168, 1, 183], 9898).into();
        bind_metrics(addr);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let channel = ChannelMetrics::new("operations".to_string(), None);
        let channel2 = ChannelMetrics::new("second_channel".to_string(), None);

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

    // #[test]
    // fn test_channel_size() {
    //     let channel = ChannelMetrics::new("test_size".to_string(), None);

    //     channel.send(1).unwrap();
    //     channel.send(2).unwrap();
    //     assert_eq!(channel.len(), 2);
    // }
}
