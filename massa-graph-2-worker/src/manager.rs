use massa_graph::error::GraphResult;
use massa_graph_2_exports::GraphManager;
use std::thread::JoinHandle;

pub struct GraphManagerImpl {
    pub thread_graph: JoinHandle<()>,
}

impl GraphManager for GraphManagerImpl {
    fn stop(&mut self) {
        todo!()
    }
}
