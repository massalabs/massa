use massa_graph_2_exports::{GraphChannels, GraphConfig, GraphController, GraphManager};
use std::sync::mpsc;
use std::thread::spawn;

use crate::commands::GraphCommand;
use crate::controller::GraphControllerImpl;
use crate::manager::GraphManagerImpl;

pub struct GraphWorker {
    pub command_receiver: mpsc::Receiver<GraphCommand>,
}

impl GraphWorker {
    pub fn new(command_receiver: mpsc::Receiver<GraphCommand>) -> Self {
        Self { command_receiver }
    }
}

pub fn start_graph_worker(
    config: GraphConfig,
    channels: GraphChannels,
    clock_compensation: i64,
) -> (Box<dyn GraphController>, Box<dyn GraphManager>) {
    let (tx, rx) = mpsc::sync_channel(10);

    let graph_thread = spawn(move || {
        let graph_worker = GraphWorker::new(rx);
        todo!();
    });

    let manager = GraphManagerImpl {
        thread_graph: graph_thread,
    };

    let controller = GraphControllerImpl { command_sender: tx };

    (Box::new(controller), Box::new(manager))
}
