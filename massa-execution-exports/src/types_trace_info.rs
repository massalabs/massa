// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! # General description
//!
//! 'execution-trace' is a Rust feature that can be enabled for the execution module to provide
//! trace of coin transfers.
//!
//! Note: that the 'execution-trace' feature is also present in [massa-sc-runtime](https://github.com/massalabs/massa-sc-runtime).
//!
//! # Transfers collected
//!
//! * TX transfers: Massa coins transferred by regular operation.
//! * SC transfers: Massa coins transferred by various ABI calls.
//!   * Transfers are nested (if happened during a SC call or localCall).
//!
//! Note: in massa-sc-runtime, all SC ABI are collected and stored
//! (see [assembly_script_get_balance](https://github.com/massalabs/massa-sc-runtime/blob/main/src/as_execution/abi.rs#L149)
//! Note 2: there is a filter in get_slot_transfers functions (json-rpc get_slots_transfers & grpc get_slot_transfers) to return only the coin transfers.
//!
//! # API
//!
//! * json-rpc public api:
//!   * get_slots_transfers
//! * grpc public api:
//!   * get_slot_transfers
//! * grpc public stream api:
//!   * new_slot_transfers
//!   * new_slot_abi_call_stacks
//!
//! # Usage
//!
//! The 'execution-trace' feature was originally developed for the [transfer-indexer](https://github.com/massalabs/transfers-indexer)
//! which is used as a starting point for exchange (CEX) integration.
//!
//! # Tests
//!
//! * Unit tests in massa-execution-worker

#[cfg(feature = "execution-trace")]
use std::collections::VecDeque;

#[cfg(feature = "execution-trace")]
use massa_models::{
    address::Address, amount::Amount, operation::OperationId, prehash::PreHashMap, slot::Slot,
};

#[cfg(feature = "execution-trace")]
pub use massa_sc_runtime::{
    AbiTrace as SCRuntimeAbiTrace, AbiTraceType as SCRuntimeAbiTraceType,
    AbiTraceValue as SCRuntimeAbiTraceValue,
};

#[cfg(feature = "execution-trace")]
use serde::Serialize;

#[cfg(feature = "execution-trace")]
#[derive(Debug, Clone, Serialize)]
/// Structure for all abi calls in a slot
pub struct SlotAbiCallStack {
    /// Slot
    pub slot: Slot,
    /// asc call stacks
    pub asc_call_stacks: Vec<Vec<AbiTrace>>,
    /// operation call stacks
    pub operation_call_stacks: PreHashMap<OperationId, Vec<AbiTrace>>,
}

#[cfg(feature = "execution-trace")]
#[derive(Debug, Clone, Serialize)]
/// structure describing a transfer
pub struct Transfer {
    /// From
    pub from: Address,
    /// To
    pub to: Address,
    /// Amount
    pub amount: Amount,
    /// Effective received amount
    pub effective_received_amount: Amount,
    /// operation id
    pub op_id: OperationId,
    /// success or not
    pub succeed: bool,
    /// Fee
    pub fee: Amount,
}

#[cfg(feature = "execution-trace")]
/// A trace of an abi call + its parameters + the result
#[derive(Debug, Clone, Serialize)]
pub struct AbiTrace {
    /// Abi name
    pub name: String,
    /// Abi parameters
    pub parameters: Vec<SCRuntimeAbiTraceValue>,
    /// Abi return value
    pub return_value: SCRuntimeAbiTraceType,
    /// Abi sub calls
    pub sub_calls: Option<Vec<AbiTrace>>,
}

#[cfg(feature = "execution-trace")]
impl From<SCRuntimeAbiTrace> for AbiTrace {
    fn from(trace: SCRuntimeAbiTrace) -> Self {
        Self {
            name: trace.name,
            parameters: trace.params,
            return_value: trace.return_value,
            sub_calls: trace.sub_calls.map(|sub_calls| {
                sub_calls
                    .into_iter()
                    .map(|sub_call| sub_call.into())
                    .collect()
            }),
        }
    }
}

#[cfg(feature = "execution-trace")]
impl AbiTrace {
    /// Flatten and filter for abi names in an AbiTrace
    pub fn flatten_filter(&self, abi_names: &[String]) -> Vec<&Self> {
        let mut filtered: Vec<&Self> = Default::default();
        let mut to_process: VecDeque<&Self> = vec![self].into();

        while !to_process.is_empty() {
            let t = to_process.pop_front();
            if let Some(trace) = t {
                if abi_names.iter().find(|t| *(*t) == trace.name).is_some() {
                    // filtered.extend(&trace)
                    filtered.push(trace);
                }

                if let Some(sub_call) = &trace.sub_calls {
                    for sc in sub_call.iter().rev() {
                        to_process.push_front(sc);
                    }
                }
            }
        }

        filtered
    }

    /// This function assumes that the abi trace is a transfer.
    /// Calling this function on a non-transfer abi trace will have undefined behavior.
    pub fn parse_transfer(&self) -> (String, String, u64) {
        let t_from = self
            .parameters
            .iter()
            .find_map(|p| {
                if p.name == "from_address" {
                    if let SCRuntimeAbiTraceType::String(v) = &p.value {
                        return Some(v.clone());
                    }
                }
                None
            })
            .unwrap_or_default();
        let t_to = self
            .parameters
            .iter()
            .find_map(|p| {
                if p.name == "to_address" {
                    if let SCRuntimeAbiTraceType::String(v) = &p.value {
                        return Some(v.clone());
                    }
                }
                None
            })
            .unwrap_or_default();
        let t_amount = self
            .parameters
            .iter()
            .find_map(|p| {
                if p.name == "raw_amount" {
                    if let SCRuntimeAbiTraceType::U64(v) = &p.value {
                        return Some(*v);
                    }
                }
                None
            })
            .unwrap_or_default();
        (t_from, t_to, t_amount)
    }
}
