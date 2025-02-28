use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use massa_db_exports::{MassaDBConfig, MassaDBController, ShareableMassaDBController};
use massa_db_worker::MassaDB;
use massa_final_state::MockFinalStateController;
use massa_models::{
    config::{
        MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE, THREAD_COUNT,
    },
    node::NodeId,
    streaming_step::StreamingStep,
};
use massa_test_framework::TestUniverse;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use num::rational::Ratio;
use parking_lot::RwLock;
use tempfile::TempDir;

use crate::{
    client::{bootstrap_from_server, connect_to_server, MockBSConnector},
    BootstrapClientMessage, BootstrapConfig, BootstrapError, GlobalBootstrapState,
};

pub struct BootstrapClientForeignControllers {
    pub final_state_controller: Arc<RwLock<MockFinalStateController>>,
    pub bs_connector: MockBSConnector,
}

impl BootstrapClientForeignControllers {
    pub fn new_with_mocks() -> Self {
        Self {
            final_state_controller: Arc::new(RwLock::new(MockFinalStateController::new())),
            bs_connector: MockBSConnector::new(),
        }
    }
}

pub struct BootstrapClientTestUniverse {
    controllers: BootstrapClientForeignControllers,
    database: ShareableMassaDBController,
    config: BootstrapConfig,
    pub(crate) global_bootstrap_state: GlobalBootstrapState,
}

impl TestUniverse for BootstrapClientTestUniverse {
    type Config = BootstrapConfig;
    type ForeignControllers = BootstrapClientForeignControllers;

    fn new(controllers: Self::ForeignControllers, config: Self::Config) -> Self {
        let global_bootstrap_state =
            GlobalBootstrapState::new(controllers.final_state_controller.clone());
        let disk_ledger_client = TempDir::new().expect("cannot create temp directory");
        let database = Arc::new(RwLock::new(Box::new(MassaDB::new(MassaDBConfig {
            path: disk_ledger_client.path().to_path_buf(),
            max_history_length: 100,
            max_versioning_elements_size: MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE as usize,
            max_final_state_elements_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE as usize,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
            enable_metrics: false,
        }))
            as Box<(dyn MassaDBController + 'static)>));
        controllers
            .final_state_controller
            .write()
            .expect_set_last_start_period()
            .returning(move |_| ());
        controllers
            .final_state_controller
            .write()
            .expect_set_last_slot_before_downtime()
            .returning(move |_| ());
        controllers
            .final_state_controller
            .write()
            .expect_get_database()
            .return_const(database.clone());
        let client_mip_store = MipStore::try_from_db(
            database.clone(),
            MipStatsConfig {
                block_count_considered: 100,
                warn_announced_version_ratio: Ratio::new(1, 2),
            },
        )
        .unwrap();
        controllers
            .final_state_controller
            .write()
            .expect_get_mip_store_mut()
            .returning(move || client_mip_store.clone());
        let universe = Self {
            controllers,
            config,
            global_bootstrap_state,
            database,
        };
        universe.initialize();
        universe
    }
}

impl BootstrapClientTestUniverse {
    pub fn launch_bootstrap(
        &mut self,
        remote_port: u16,
        remote_node_id: NodeId,
    ) -> Result<(), BootstrapError> {
        let remote_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), remote_port);
        self.controllers
            .bs_connector
            .expect_connect_timeout()
            .times(1)
            .returning(move |_, _| Ok(std::net::TcpStream::connect(remote_addr).unwrap()));
        //TODO: Maybe move it out of this
        let version = "BOOT.1.0".parse().unwrap();
        let mut next_bootstrap_message: BootstrapClientMessage =
            BootstrapClientMessage::AskBootstrapPart {
                last_slot: None,
                last_state_step: StreamingStep::Started,
                last_versioning_step: StreamingStep::Started,
                last_consensus_step: StreamingStep::Started,
                send_last_start_period: true,
            };

        let mut conn = connect_to_server(
            &mut self.controllers.bs_connector,
            &self.config,
            &remote_addr,
            &remote_node_id.get_public_key(),
            Some(self.config.rate_limit),
        )
        .unwrap();
        bootstrap_from_server(
            &self.config,
            &mut conn,
            &mut next_bootstrap_message,
            &mut self.global_bootstrap_state,
            version,
        )
    }

    //TODO: Add consensus blocks and peers
    pub fn compare_database(&self, other_database: ShareableMassaDBController) {
        assert_eq!(
            self.database.read().get_entire_database(),
            other_database.read().get_entire_database()
        );
    }
}
