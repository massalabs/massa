use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use massa_consensus_exports::{
    bootstrapable_graph::BootstrapableGraph, MockConsensusControllerWrapper,
};
use massa_db_exports::{DBBatch, MassaDBConfig, MassaDBController, ShareableMassaDBController};
use massa_db_worker::MassaDB;
use massa_final_state::MockFinalStateController;
use massa_ledger_exports::{LedgerChanges, LedgerConfig, LedgerController};
use massa_ledger_worker::FinalLedger;
use massa_metrics::MassaMetrics;
use massa_models::{
    address::Address,
    amount::Amount,
    bytecode::Bytecode,
    config::{
        MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE, MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE,
        MAX_DATASTORE_KEY_LENGTH, MAX_DATASTORE_VALUE_LENGTH, THREAD_COUNT,
    },
    datastore::Datastore,
    prehash::PreHashSet,
    streaming_step::StreamingStep,
};
use massa_protocol_exports::{BootstrapPeers, MockProtocolControllerWrapper};
use massa_signature::KeyPair;
use massa_test_framework::TestUniverse;
use mio::{Poll, Token, Waker};
use mockall::Sequence;
use parking_lot::RwLock;
use tempfile::{NamedTempFile, TempDir};

use crate::{
    listener::{BootstrapListenerStopHandle, MockBootstrapTcpListener, PollEvent},
    start_bootstrap_server, BootstrapConfig, BootstrapError, BootstrapManager,
};

pub struct BootstrapServerForeignControllers {
    pub final_state_controller: Arc<RwLock<MockFinalStateController>>,
    pub consensus_controller: MockConsensusControllerWrapper,
    pub protocol_controller: MockProtocolControllerWrapper,
    pub listener: MockBootstrapTcpListener,
    pub server_keypair: KeyPair,
    pub database: ShareableMassaDBController,
}

impl BootstrapServerForeignControllers {
    pub fn new_with_mocks() -> Self {
        let disk_ledger_server = TempDir::new().expect("cannot create temp directory");
        let database = Arc::new(RwLock::new(Box::new(MassaDB::new(MassaDBConfig {
            path: disk_ledger_server.path().to_path_buf(),
            max_history_length: 100,
            max_versioning_elements_size: MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE as usize,
            max_final_state_elements_size: MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE as usize,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        }))
            as Box<(dyn MassaDBController + 'static)>));
        Self {
            final_state_controller: Arc::new(RwLock::new(MockFinalStateController::new())),
            consensus_controller: MockConsensusControllerWrapper::new(),
            protocol_controller: MockProtocolControllerWrapper::new(),
            listener: MockBootstrapTcpListener::new(),
            server_keypair: KeyPair::generate(0).unwrap(),
            database,
        }
    }
}

pub struct BootstrapServerTestUniverse {
    pub module_manager: Option<Box<BootstrapManager>>,
    pub database: ShareableMassaDBController,
}

impl TestUniverse for BootstrapServerTestUniverse {
    type Config = BootstrapConfig;
    type ForeignControllers = BootstrapServerForeignControllers;

    fn new(controllers: Self::ForeignControllers, config: Self::Config) -> Self {
        let version = "BOOT.1.0".parse().unwrap();
        let poll = Poll::new().unwrap();
        let waker = BootstrapListenerStopHandle(Waker::new(poll.registry(), Token(10)).unwrap());
        let module_manager = start_bootstrap_server(
            controllers.listener,
            waker,
            Box::new(controllers.consensus_controller),
            Box::new(controllers.protocol_controller),
            controllers.final_state_controller,
            config,
            controllers.server_keypair,
            version,
            MassaMetrics::new(
                false,
                "0.0.0.0:31248".parse().unwrap(),
                THREAD_COUNT,
                Duration::from_secs(5),
            )
            .0,
        )
        .unwrap();
        let universe = Self {
            module_manager: Some(Box::new(module_manager)),
            database: controllers.database,
        };
        universe.initialize();
        universe
    }
}

impl Drop for BootstrapServerTestUniverse {
    fn drop(&mut self) {
        self.module_manager.take().unwrap().stop().unwrap();
    }
}

pub struct BootstrapServerTestUniverseBuilder {
    controllers: BootstrapServerForeignControllers,
    config: BootstrapConfig,
    final_ledger: FinalLedger,
    socket_addr: SocketAddr,
    accept_error: bool,
}

impl Default for BootstrapServerTestUniverseBuilder {
    fn default() -> Self {
        let mut controllers = BootstrapServerForeignControllers::new_with_mocks();
        let file: NamedTempFile = NamedTempFile::new().unwrap();
        let final_ledger = FinalLedger::new(
            LedgerConfig {
                thread_count: THREAD_COUNT,
                initial_ledger_path: file.path().to_path_buf(),
                max_key_length: MAX_DATASTORE_KEY_LENGTH,
                max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
            },
            controllers.database.clone(),
        );
        controllers
            .final_state_controller
            .write()
            .expect_get_database()
            .return_const(controllers.database.clone());
        controllers
            .final_state_controller
            .write()
            .expect_get_last_start_period()
            .returning(move || 0);
        controllers
            .final_state_controller
            .write()
            .expect_get_last_slot_before_downtime()
            .return_const(None);
        controllers
            .consensus_controller
            .set_expectations(|consensus_controller| {
                consensus_controller.expect_get_bootstrap_part().returning(
                    move |last_consensus_step, _slot| match last_consensus_step {
                        StreamingStep::Started => Ok((
                            BootstrapableGraph {
                                final_blocks: vec![],
                            },
                            PreHashSet::default(),
                            StreamingStep::Ongoing(PreHashSet::default()),
                        )),
                        _ => Ok((
                            BootstrapableGraph {
                                final_blocks: vec![],
                            },
                            PreHashSet::default(),
                            StreamingStep::Finished(None),
                        )),
                    },
                );
            });
        controllers
            .protocol_controller
            .set_expectations(|protocol_controller| {
                protocol_controller
                    .expect_get_bootstrap_peers()
                    .returning(|| Ok(BootstrapPeers(vec![])));
            });
        Self {
            controllers,
            config: BootstrapConfig::default(),
            final_ledger,
            socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8069),
            accept_error: false,
        }
    }
}

impl BootstrapServerTestUniverseBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_address_balance(mut self, address: &Address, amount: Amount) -> Self {
        let mut ledger_changes = LedgerChanges::default();
        ledger_changes.set_balance(*address, amount);
        let mut batch = DBBatch::default();
        let versioning_batch = DBBatch::default();
        self.final_ledger
            .apply_changes_to_batch(ledger_changes, &mut batch, 1);
        self.controllers
            .database
            .write()
            .write_batch(batch, versioning_batch, None);
        self
    }

    pub fn set_bytecode(mut self, address: &Address, bytecode: Bytecode) -> Self {
        let mut ledger_changes = LedgerChanges::default();
        ledger_changes.set_bytecode(*address, bytecode);
        let mut batch = DBBatch::default();
        let versioning_batch = DBBatch::default();
        self.final_ledger
            .apply_changes_to_batch(ledger_changes, &mut batch, 1);
        self.controllers
            .database
            .write()
            .write_batch(batch, versioning_batch, None);
        self
    }

    pub fn set_datastore(mut self, address: &Address, datastore: Datastore) -> Self {
        let mut ledger_changes = LedgerChanges::default();
        for (key, value) in datastore.into_iter() {
            ledger_changes.set_data_entry(*address, key, value);
        }
        let mut batch = DBBatch::default();
        let versioning_batch = DBBatch::default();
        self.final_ledger
            .apply_changes_to_batch(ledger_changes, &mut batch, 1);
        self.controllers
            .database
            .write()
            .write_batch(batch, versioning_batch, None);
        self
    }

    pub fn set_keypair(mut self, keypair: &KeyPair) -> Self {
        self.controllers.server_keypair = keypair.clone();
        self
    }

    pub fn set_port(mut self, port: u16) -> Self {
        self.socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        self
    }

    pub fn set_accept_error(mut self, accept_error: bool) -> Self {
        self.accept_error = accept_error;
        self
    }

    pub fn set_config(mut self, config: BootstrapConfig) -> Self {
        self.config = config;
        self
    }

    pub fn build(mut self) -> BootstrapServerTestUniverse {
        //TODO: Add possibility to chain bootstrap
        let listener = std::net::TcpListener::bind(self.socket_addr).unwrap();
        let mut sequence = Sequence::new();
        if self.accept_error {
            self.controllers
                .listener
                .expect_poll()
                .times(1)
                // Mock the `accept` method here by receiving from the listen-loop thread
                .returning(move || {
                    Err(BootstrapError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "mocked error",
                    )))
                })
                .in_sequence(&mut sequence);
        } else {
            // Due to the limitations of the mocking system, the listener must loop-accept in a dedicated
            // thread. We use a channel to make the connection available in the mocked `accept` method
            let (conn_tx, conn_rx) = std::sync::mpsc::sync_channel(100);
            std::thread::Builder::new()
                .name("mock-listen-loop".to_string())
                .spawn(move || loop {
                    conn_tx.send(listener.accept().unwrap()).unwrap()
                })
                .unwrap();
            self.controllers
                .listener
                .expect_poll()
                .times(1)
                // Mock the `accept` method here by receiving from the listen-loop thread
                .returning(move || Ok(PollEvent::NewConnections(vec![conn_rx.recv().unwrap()])))
                .in_sequence(&mut sequence);
        }
        self.controllers
            .listener
            .expect_poll()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(PollEvent::Stop));
        BootstrapServerTestUniverse::new(self.controllers, self.config)
    }
}
