use consensus::{
    BlockGraphExport, ConsensusCommand, ConsensusCommandSender, ExportDiscardedBlocks,
};
use crypto::{
    hash::Hash,
    signature::{PublicKey, Signature, SignatureEngine},
};
use models::block::Block;
use tokio::{sync::mpsc, task::JoinHandle};

use std::collections::HashMap;

const CHANNEL_SIZE: usize = 16;

#[derive(Clone)]
pub struct ConsensusMockData {
    pub graph: BlockGraphExport,
    pub dummy_signature: Signature,
    pub dummy_creator: PublicKey,
}

impl ConsensusMockData {
    pub fn new() -> Self {
        let signature_engine = SignatureEngine::new();
        let private_key = SignatureEngine::generate_random_private_key();
        let public_key = signature_engine.derive_public_key(&private_key);
        let hash = Hash::hash("default_val".as_bytes());
        let dummy_signature = signature_engine
            .sign(&hash, &private_key)
            .expect("could not sign");
        let best_parents = vec![];
        let discarded_blocks = ExportDiscardedBlocks {
            map: HashMap::new(),
        };
        ConsensusMockData {
            graph: BlockGraphExport {
                genesis_blocks: Vec::new(),
                active_blocks: HashMap::new(),
                discarded_blocks,
                best_parents,
                latest_final_blocks_periods: Vec::new(),
                gi_head: HashMap::new(),
                max_cliques: Vec::new(),
            },
            dummy_signature,
            dummy_creator: public_key.clone(),
        }
    }

    pub fn add_active_blocks(&mut self, hash: Hash, block: Block) {
        self.graph.active_blocks.insert(
            hash,
            consensus::ExportCompiledBlock {
                block: block.header,
                children: vec![],
            },
        );
    }
}

pub fn start_mock_consensus(data: ConsensusMockData) -> (JoinHandle<()>, ConsensusCommandSender) {
    let (consensus_command_tx, mut consensus_command_rx) =
        mpsc::channel::<ConsensusCommand>(CHANNEL_SIZE);

    let join_handle = tokio::spawn(async move {
        loop {
            match consensus_command_rx.recv().await {
                None => break,
                Some(ConsensusCommand::GetBlockGraphStatus(sender)) => {
                    sender.send(data.graph.clone()).unwrap()
                }
                Some(ConsensusCommand::GetActiveBlock(hash, sender)) => {
                    sender
                        .send(data.graph.active_blocks.get(&hash).map(|cb| Block {
                            header: cb.block.clone(),
                            operations: vec![],
                            signature: data.dummy_signature.clone(),
                        }))
                        .unwrap();
                }
                Some(ConsensusCommand::GetSelectionDraws(_from_slot, _to_slot, sender)) => {
                    sender
                        .send(Ok(vec![((0u64, 0u8), data.dummy_creator)]))
                        .unwrap();
                }
            }
        }
    });

    (join_handle, ConsensusCommandSender(consensus_command_tx))
}
