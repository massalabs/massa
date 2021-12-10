use crate::sce_ledger::{SCELedger, SCELedgerChanges, SCELedgerStep};
/// Define types used while executing block bytecodes
use massa_models::{Address, Amount, OperationContent, OperationType};
use massa_models::{Block, BlockId, Slot};
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::Mutex;

pub type StepHistory = VecDeque<(Slot, Option<BlockId>, SCELedgerChanges)>;
pub type Bytecode = Vec<u8>;

/// Operation should be used to communicate with the VM, TODO, it doesn't need everything in.
/// TODO May be the max_gas, the module and the sender address are enough
pub(crate) struct OperationSC {
    pub _module: Bytecode,
    pub max_gas: u64,
    pub coins: Amount,
    pub gas_price: Amount,
    pub sender: Address,
}

#[derive(Clone)]
/// Stateful context, providing an execution context to massa between module executions
pub(crate) struct ExecutionContext {
    pub ledger_step: SCELedgerStep,
    pub max_gas: u64,
    pub coins: Amount,
    pub gas_price: Amount,
    pub slot: Slot,
    pub opt_block_id: Option<BlockId>,
    pub opt_block_creator_addr: Option<Address>,
    pub call_stack: VecDeque<Address>,
}

#[derive(Clone)]
pub(crate) struct ExecutionStep {
    pub slot: Slot,
    // TODO add pos_seed for RNG seeding
    // TODO add pos_draws to list the draws (block and endorsement creators) for that slot
    pub block: Option<(BlockId, Block)>, // None if miss
}

impl ExecutionContext {
    pub fn new(ledger: SCELedger) -> Self {
        let final_ledger = Arc::new(Mutex::new(
            ledger, // TODO bootstrap
        ));
        ExecutionContext {
            ledger_step: SCELedgerStep {
                final_ledger,
                cumulative_history_changes: Default::default(),
                caused_changes: Default::default(),
            },
            max_gas: Default::default(),
            coins: Default::default(),
            gas_price: Default::default(),
            slot: Slot::new(0, 0),
            opt_block_id: Default::default(),
            opt_block_creator_addr: Default::default(),
            call_stack: Default::default(),
        }
    }
}

impl TryFrom<OperationContent> for OperationSC {
    type Error = ();

    fn try_from(content: OperationContent) -> Result<Self, Self::Error> {
        match content.op {
            OperationType::ExecuteSC {
                data,
                max_gas,
                coins,
                gas_price,
            } => {
                Ok(OperationSC {
                    _module: data, // todo use the external lib to check if module is valid
                    max_gas,
                    coins,
                    gas_price,
                    sender: Address::from_public_key(&content.sender_public_key).unwrap(),
                })
            }
            _ => Err(()),
        }
    }
}

impl From<StepHistory> for SCELedgerChanges {
    fn from(step: StepHistory) -> Self {
        let mut ret = SCELedgerChanges::default();
        step.iter().for_each(|(_, _, step_changes)| {
            ret.apply_changes(step_changes);
        });
        ret
    }
}
