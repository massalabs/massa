use lazy_static::lazy_static;
use prometheus::{register_int_gauge, Gauge, IntGauge};

mod server;

// TODO load only if feature metrics is enabled
lazy_static! {


    static ref IN_CONNECTIONS: IntGauge = register_int_gauge!("in_connections", "active in connections").unwrap();

    static ref OUT_CONNECTIONS: IntGauge = register_int_gauge!("out_connections", "active out connections").unwrap();

    static ref OPERATIONS_COUNTER: IntGauge = register_int_gauge!("operations_counter", "operations counter len").unwrap();
    static ref BLOCKS_COUNTER: IntGauge = register_int_gauge!("blocks_counter", "blocks counter len").unwrap();
    static ref ENDORSEMENTS_COUNTER: IntGauge = register_int_gauge!("endorsements_counter", "endorsements counter len").unwrap();


    // static ref A_INT_GAUGE: IntGauge = register_int_gauge!("A_int_gauge", "foobar").unwrap();
}

pub fn set_connections(in_connections: usize, out_connections: usize) {
    IN_CONNECTIONS.set(in_connections as i64);
    OUT_CONNECTIONS.set(out_connections as i64);
}

pub fn inc_blocks_counter() {
    BLOCKS_COUNTER.inc();
}

pub fn dec_blocks_counter() {
    BLOCKS_COUNTER.dec();
}

pub fn inc_endorsements_counter() {
    ENDORSEMENTS_COUNTER.inc();
}

pub fn dec_endorsements_counter() {
    ENDORSEMENTS_COUNTER.dec();
}

pub fn inc_operations_counter() {
    OPERATIONS_COUNTER.inc();
}

pub fn dec_operations_counter() {
    OPERATIONS_COUNTER.dec();
}

#[derive(Clone)]
pub struct MassaMetrics {
    consensus_vec: Vec<Gauge>,

    active_in_connections: IntGauge,
    active_out_connections: IntGauge,

    // blocks_counter: IntGauge,
    // endorsements_counter: IntGauge,
    // operations_counter: IntGauge,
    active_cursor_thread: IntGauge,
    active_cursor_period: IntGauge,

    final_cursor_thread: IntGauge,
    final_cursor_period: IntGauge,
}

impl MassaMetrics {
    pub fn new(nb_thread: u8) -> Self {
        let mut consensus_vec = vec![];
        for i in 0..nb_thread {
            let gauge = Gauge::new(
                format!("consensus_thread_{}", i),
                "consensus thread actual period",
            )
            .expect("Failed to create gauge");

            prometheus::register(Box::new(gauge.clone())).expect("Failed to register gauge");
            consensus_vec.push(gauge);
        }

        // active cursor
        let active_cursor_thread =
            IntGauge::new("active_cursor_thread", "execution active cursor thread").unwrap();
        let active_cursor_period =
            IntGauge::new("active_cursor_period", "execution active cursor period").unwrap();
        prometheus::register(Box::new(active_cursor_thread.clone()))
            .expect("Failed to register gauge");
        prometheus::register(Box::new(active_cursor_period.clone()))
            .expect("Failed to register gauge");

        // final cursor
        let final_cursor_thread =
            IntGauge::new("final_cursor_thread", "execution final cursor thread").unwrap();
        let final_cursor_period =
            IntGauge::new("final_cursor_period", "execution final cursor period").unwrap();
        prometheus::register(Box::new(final_cursor_thread.clone()))
            .expect("Failed to register gauge");
        prometheus::register(Box::new(final_cursor_period.clone()))
            .expect("Failed to register gauge");

        // TODO addr from config
        let addr = "0.0.0.0:9898".parse().unwrap();
        server::bind_metrics(addr);

        // // block counter
        // let blocks_counter = IntGauge::new("blocks_counter", "block counter len").unwrap();
        // prometheus::register(Box::new(blocks_counter.clone())).expect("Failed to register gauge");

        // // endorsement counter
        // let endorsements_counter =
        //     IntGauge::new("endorsements_counter", "endorsements counter len").unwrap();
        // prometheus::register(Box::new(endorsements_counter.clone()))
        //     .expect("Failed to register gauge");

        // operation counter
        // let operations_counter =
        //     IntGauge::new("operations_counter", "operations counter len").unwrap();
        // prometheus::register(Box::new(operations_counter.clone()))
        //     .expect("Failed to register gauge");

        // active connections IN
        let active_in_connections =
            IntGauge::new("active_in_connections", "active connections IN len").unwrap();
        prometheus::register(Box::new(active_in_connections.clone()))
            .expect("Failed to register gauge");

        // active connections OUT
        let active_out_connections =
            IntGauge::new("active_out_connections", "active connections OUT len").unwrap();
        prometheus::register(Box::new(active_out_connections.clone()))
            .expect("Failed to register gauge");

        MassaMetrics {
            consensus_vec,
            active_in_connections,
            active_out_connections,
            // blocks_counter,
            // endorsements_counter,
            // operations_counter,
            active_cursor_thread,
            active_cursor_period,
            final_cursor_thread,
            final_cursor_period,
        }
    }

    // pub fn inc_blocks_counter(&self) {
    //     self.blocks_counter.inc();
    // }

    // pub fn dec_blocks_counter(&self) {
    //     self.blocks_counter.dec();
    // }

    // pub fn inc_endorsements_counter(&self) {
    //     self.endorsements_counter.inc();
    // }

    // pub fn dec_endorsements_counter(&self) {
    //     self.endorsements_counter.dec();
    // }

    // pub fn inc_operations_counter(&self) {
    //     self.operations_counter.inc();
    // }

    // pub fn dec_operations_counter(&self) {
    //     self.operations_counter.dec();
    // }

    pub fn set_active_connections(&self, in_connections: usize, out_connections: usize) {
        self.active_in_connections.set(in_connections as i64);
        self.active_out_connections.set(out_connections as i64);
    }

    pub fn set_active_cursor(&self, period: u64, thread: u8) {
        self.active_cursor_thread.set(thread as i64);
        self.active_cursor_period.set(period as i64);
    }

    pub fn set_final_cursor(&self, period: u64, thread: u8) {
        self.final_cursor_thread.set(thread as i64);
        self.final_cursor_period.set(period as i64);
    }

    pub fn set_consensus_period(&self, thread: usize, period: u64) {
        self.consensus_vec[thread].set(period as f64);
    }
}
// mod test {
//     use massa_channel::MassaChannel;

//     use crate::start_metrics_server;

//     #[tokio::test]
//     async fn test_channel_metrics() {
//         let addr = ([192, 168, 1, 183], 9898).into();

//         start_metrics_server(addr);
//         std::thread::sleep(std::time::Duration::from_millis(500));
//         let (sender, receiver) = MassaChannel::new("operations".to_string(), None);

//         let (sender2, receiver2) = MassaChannel::new("second_channel".to_string(), None);

//         sender2.send("hello_world".to_string()).unwrap();
//         let data = receiver2.recv().unwrap();
//         assert_eq!(data, "hello_world".to_string());

//         for i in 0..100 {
//             sender.send(i).unwrap();
//         }

//         for _i in 0..20 {
//             receiver.recv().unwrap();
//         }

//         assert_eq!(receiver.len(), 80);
//         std::thread::sleep(std::time::Duration::from_secs(5));
//         drop(sender2);
//         drop(receiver2);
//         std::thread::sleep(std::time::Duration::from_secs(100));
//     }

//     #[tokio::test]
//     async fn test_channel() {
//         let addr = ([192, 168, 1, 183], 9898).into();

//         start_metrics_server(addr);
//         std::thread::sleep(std::time::Duration::from_millis(500));

//         let (sender, receiver) = MassaChannel::new("test2".to_string(), None);

//         let cloned = receiver.clone();

//         sender.send("msg".to_string()).unwrap();

//         std::thread::spawn(move || {
//             dbg!("spawned");

//             loop {
//                 dbg!("loop");
//                 dbg!(receiver.recv().unwrap());
//                 std::thread::sleep(std::time::Duration::from_secs(1));
//             }
//         });
//         std::thread::sleep(std::time::Duration::from_secs(2));
//         std::thread::spawn(move || {
//             std::thread::sleep(std::time::Duration::from_secs(5));

//             drop(sender);
//         });

//         std::thread::sleep(std::time::Duration::from_secs(20));
//     }
// }
