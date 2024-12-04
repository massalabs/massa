// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaPublicGrpc;
use crate::{EndorsementDraw, SlotDraw, SlotRange};

use itertools::{izip, Itertools};
use massa_execution_exports::mapping_grpc::{
    to_event_filter, to_execution_query_response, to_querystate_filter,
};
use massa_execution_exports::{
    ExecutionQueryRequest, ExecutionStackElement, ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::block::{Block, BlockGraphStatus};
use massa_models::block_id::BlockId;
use massa_models::config::CompactConfig;
use massa_models::datastore::DatastoreDeserializer;
use massa_models::endorsement::{EndorsementId, SecureShareEndorsement};
use massa_models::operation::{OperationId, SecureShareOperation};
use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_models::slot::Slot;
use massa_models::timeslots::get_latest_block_slot_at_timestamp;
use massa_proto_rs::massa::api::v1::{self as grpc_api};
use massa_proto_rs::massa::model::v1::{self as grpc_model, read_only_execution_call};
use massa_serialization::{DeserializeError, Deserializer};
use massa_time::MassaTime;
use massa_versioning::versioning_factory::{FactoryStrategy, VersioningFactory};
use std::collections::HashSet;
use std::str::FromStr;

#[cfg(feature = "execution-trace")]
use massa_execution_exports::types_trace_info::AbiTrace;
#[cfg(feature = "execution-trace")]
use massa_proto_rs::massa::api::v1::abi_call_stack_element_parent::CallStackElement;
#[cfg(feature = "execution-trace")]
use massa_proto_rs::massa::api::v1::{
    AbiCallStack, AbiCallStackElement, AbiCallStackElementCall, AbiCallStackElementParent,
    AscabiCallStack, GetOperationAbiCallStacksResponse, GetSlotAbiCallStacksResponse,
    OperationAbiCallStack, SlotAbiCallStacks, TransferInfo, TransferInfos,
};

/// Execute read only call (function or bytecode)
pub(crate) fn execute_read_only_call(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::ExecuteReadOnlyCallRequest>,
) -> Result<grpc_api::ExecuteReadOnlyCallResponse, GrpcError> {
    let call: grpc_model::ReadOnlyExecutionCall = request
        .into_inner()
        .call
        .ok_or_else(|| GrpcError::InvalidArgument("no call provided".to_string()))?;

    let caller_address = match call.caller_address {
        Some(addr) => Address::from_str(&addr)?,
        None => {
            let now = MassaTime::now();
            let keypair = grpc.keypair_factory.create(&(), FactoryStrategy::At(now))?;
            Address::from_public_key(&keypair.get_public_key())
        }
    };

    let mut call_stack = Vec::new();
    let mut coins = None;
    let target = if let Some(call_target) = call.target {
        match call_target {
            read_only_execution_call::Target::BytecodeCall(value) => {
                let op_datastore = if value.operation_datastore.is_empty() {
                    None
                } else {
                    let deserializer = DatastoreDeserializer::new(
                        grpc.grpc_config.max_op_datastore_entry_count,
                        grpc.grpc_config.max_op_datastore_key_length,
                        grpc.grpc_config.max_op_datastore_value_length,
                    );
                    match deserializer.deserialize::<DeserializeError>(&value.operation_datastore) {
                        Ok((_, deserialized)) => Some(deserialized),
                        Err(e) => {
                            return Err(GrpcError::InvalidArgument(format!(
                                "Datastore deserializing error: {}",
                                e
                            )))
                        }
                    }
                };

                call_stack.push(ExecutionStackElement {
                    address: caller_address,
                    coins: Default::default(),
                    owned_addresses: vec![caller_address],
                    operation_datastore: op_datastore,
                });

                ReadOnlyExecutionTarget::BytecodeExecution(value.bytecode)
            }
            read_only_execution_call::Target::FunctionCall(call) => {
                let target_address = Address::from_str(&call.target_address)?;
                call_stack.push(ExecutionStackElement {
                    address: caller_address,
                    coins: Default::default(),
                    owned_addresses: vec![caller_address],
                    operation_datastore: None, // should always be None
                });
                call_stack.push(ExecutionStackElement {
                    address: target_address,
                    coins: Default::default(),
                    owned_addresses: vec![target_address],
                    operation_datastore: None, // should always be None
                });

                coins = call
                    .coins
                    .map(|native_amount| {
                        Amount::from_mantissa_scale(native_amount.mantissa, native_amount.scale)
                            .map_err(|_| GrpcError::InvalidArgument("invalid amount".to_string()))
                    })
                    .transpose()?;

                ReadOnlyExecutionTarget::FunctionCall {
                    target_addr: Address::from_str(&call.target_address)?,
                    target_func: call.target_function,
                    parameter: call.parameter,
                }
            }
        }
    } else {
        return Err(GrpcError::InvalidArgument(
            "no call target provided".to_string(),
        ));
    };

    let read_only_call = ReadOnlyExecutionRequest {
        max_gas: call.max_gas,
        call_stack,
        target,
        coins,
        fee: call
            .fee
            .map(|native_amount| {
                Amount::from_mantissa_scale(native_amount.mantissa, native_amount.scale)
                    .map_err(|_| GrpcError::InvalidArgument("invalid amount".to_string()))
            })
            .transpose()?,
    };

    if read_only_call
        .fee
        .unwrap_or_default()
        .checked_sub(grpc.grpc_config.minimal_fees)
        .is_none()
    {
        return Err(GrpcError::InvalidArgument(format!(
            "fee is too low provided: {} , minimal_fees required: {}",
            read_only_call.fee.unwrap_or_default(),
            grpc.grpc_config.minimal_fees
        )));
    }

    let output = grpc
        .execution_controller
        .execute_readonly_request(read_only_call)?;

    let result = grpc_model::ReadOnlyExecutionOutput {
        out: Some(output.out.into()),
        used_gas: output.gas_cost,
        call_result: output.call_result,
    };

    Ok(grpc_api::ExecuteReadOnlyCallResponse {
        output: Some(result),
    })
}

/// Get blocks
pub(crate) fn get_blocks(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetBlocksRequest>,
) -> Result<grpc_api::GetBlocksResponse, GrpcError> {
    let ids = request.into_inner().block_ids;

    if ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no block id provided".to_string(),
        ));
    }

    if ids.len() as u32 > grpc.grpc_config.max_operation_ids_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many block ids received. Only a maximum of {} block ids are accepted per request",
            grpc.grpc_config.max_block_ids_per_request
        )));
    }

    let mut block_ids: Vec<BlockId> = ids
        .into_iter()
        .take(grpc.grpc_config.max_operation_ids_per_request as usize + 1)
        .map(|id| {
            BlockId::from_str(id.as_str())
                .map_err(|_| GrpcError::InvalidArgument(format!("invalid block id: {}", id)))
        })
        .collect::<Result<_, _>>()?;

    let mut blocks: Vec<Block> = Vec::with_capacity(block_ids.len());
    {
        let block_storage_lock = grpc.storage.read_blocks();
        block_ids.retain(|id| {
            if let Some(wrapped_block) = block_storage_lock.get(id) {
                blocks.push(wrapped_block.content.clone());
                return true;
            };
            false
        });
    }

    let block_statuses = grpc.consensus_controller.get_block_statuses(&block_ids);

    let result = blocks
        .into_iter()
        .zip(block_statuses)
        .map(|(block, block_graph_status)| grpc_model::BlockWrapper {
            block: Some(block.into()),
            status: block_graph_status.into(),
        })
        .collect();

    Ok(grpc_api::GetBlocksResponse {
        wrapped_blocks: result,
    })
}

/// Get multiple datastore entries
pub(crate) fn get_datastore_entries(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetDatastoreEntriesRequest>,
) -> Result<grpc_api::GetDatastoreEntriesResponse, GrpcError> {
    let inner_req = request.into_inner();

    // return error if entry are empty
    if inner_req.filters.is_empty() {
        return Err(GrpcError::InvalidArgument("no filter provided".to_string()));
    }

    // return error if entry are too many filters for a single request
    if inner_req.filters.len() as u64 > grpc.grpc_config.max_datastore_entries_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many datastore entries received. Only a maximum of {} datastore entries are accepted per request",
            grpc.grpc_config.max_datastore_entries_per_request
        )));
    }

    let filters: Vec<(Address, Vec<u8>)> = inner_req
        .filters
        .into_iter()
        .filter_map(|filter| {
            filter.filter.and_then(|filter| match filter {
                grpc_api::get_datastore_entry_filter::Filter::AddressKey(addrs) => {
                    if let Ok(add) = &Address::from_str(&addrs.address) {
                        Some((*add, addrs.key))
                    } else {
                        None
                    }
                }
            })
        })
        .collect();

    let entries = grpc
        .execution_controller
        .get_final_and_active_data_entry(filters)
        .into_iter()
        .map(|output| grpc_model::DatastoreEntry {
            final_value: output.0.unwrap_or_default(),
            candidate_value: output.1.unwrap_or_default(),
        })
        .collect();

    Ok(grpc_api::GetDatastoreEntriesResponse {
        datastore_entries: entries,
    })
}

/// Get endorsements
pub(crate) fn get_endorsements(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetEndorsementsRequest>,
) -> Result<grpc_api::GetEndorsementsResponse, GrpcError> {
    let ids = request.into_inner().endorsement_ids;

    if ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no endorsement id provided".to_string(),
        ));
    }

    if ids.len() as u32 > grpc.grpc_config.max_endorsement_ids_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many endorsement ids received. Only a maximum of {} endorsement ids are accepted per request",
            grpc.grpc_config.max_endorsements_per_message
        )));
    }

    let mut endorsement_ids: Vec<EndorsementId> = ids
        .into_iter()
        .take(grpc.grpc_config.max_operation_ids_per_request as usize + 1)
        .map(|id| {
            EndorsementId::from_str(id.as_str())
                .map_err(|_| GrpcError::InvalidArgument(format!("invalid endorsement id: {}", id)))
        })
        .collect::<Result<_, _>>()?;

    let mut secure_share_endorsements: Vec<SecureShareEndorsement> =
        Vec::with_capacity(endorsement_ids.len());
    {
        let endorsement_storage_lock = grpc.storage.read_endorsements();
        endorsement_ids.retain(|id| {
            if let Some(wrapped_endorsement) = endorsement_storage_lock.get(id) {
                secure_share_endorsements.push(wrapped_endorsement.clone());
                return true;
            };
            false
        });
    }

    let storage_info: Vec<(SecureShareEndorsement, PreHashSet<BlockId>)> = {
        let read_blocks = grpc.storage.read_blocks();
        secure_share_endorsements
            .into_iter()
            .map(|secure_share_operation| {
                let ed_id = secure_share_operation.id;
                (
                    secure_share_operation,
                    read_blocks
                        .get_blocks_by_endorsement(&ed_id)
                        .cloned()
                        .unwrap_or_default(),
                )
            })
            .collect()
    };

    // ask pool whether it carries the endorsements
    let in_pool = grpc.pool_controller.contains_endorsements(&endorsement_ids);

    // check finality by cross-referencing Consensus and looking for final blocks that contain the endorsement
    let is_final: Vec<bool> = {
        let involved_blocks: Vec<BlockId> = storage_info
            .iter()
            .flat_map(|(_ed, bs)| bs.iter())
            .unique()
            .cloned()
            .collect();

        let involved_block_statuses = grpc
            .consensus_controller
            .get_block_statuses(&involved_blocks);

        let block_statuses: PreHashMap<BlockId, BlockGraphStatus> = involved_blocks
            .into_iter()
            .zip(involved_block_statuses)
            .collect();

        storage_info
            .iter()
            .map(|(_ed, bs)| {
                bs.iter()
                    .any(|b| block_statuses.get(b) == Some(&BlockGraphStatus::Final))
            })
            .collect()
    };

    // gather all values into a vector of EndorsementInfo instances
    let mut result: Vec<grpc_model::EndorsementWrapper> = Vec::with_capacity(endorsement_ids.len());
    let zipped_iterator = izip!(
        storage_info.into_iter(),
        in_pool.into_iter(),
        is_final.into_iter()
    );

    for ((endorsement, in_blocks), in_pool, is_final) in zipped_iterator {
        result.push(grpc_model::EndorsementWrapper {
            in_pool,
            is_final,
            in_blocks: in_blocks
                .into_iter()
                .map(|block_id| block_id.to_string())
                .collect(),
            endorsement: Some(endorsement.into()),
        });
    }

    Ok(grpc_api::GetEndorsementsResponse {
        wrapped_endorsements: result,
    })
}

/// Get the stakers
pub(crate) fn get_stakers(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetStakersRequest>,
) -> Result<grpc_api::GetStakersResponse, GrpcError> {
    let inner_req = request.into_inner();

    // min_roll, max_roll, limit
    let mut filter_opt = (None, None, None);

    // Parse the query parameters, if provided.
    inner_req
        .filters
        .iter()
        .for_each(|filter| match filter.filter {
            Some(grpc_api::stakers_filter::Filter::MinRolls(min_rolls)) => {
                filter_opt.0 = Some(min_rolls);
            }
            Some(grpc_api::stakers_filter::Filter::MaxRolls(max_rolls)) => {
                filter_opt.1 = Some(max_rolls);
            }
            Some(grpc_api::stakers_filter::Filter::Limit(limit)) => {
                filter_opt.2 = Some(limit);
            }
            None => {}
        });

    // Get the current cycle and slot.
    let now: MassaTime = MassaTime::now();

    let latest_block_slot_at_timestamp_result = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    );

    let (cur_cycle, _cur_slot) = match latest_block_slot_at_timestamp_result {
        Ok(Some(cur_slot)) if cur_slot.period <= grpc.grpc_config.last_start_period => (
            Slot::new(grpc.grpc_config.last_start_period, 0)
                .get_cycle(grpc.grpc_config.periods_per_cycle),
            cur_slot,
        ),
        Ok(Some(cur_slot)) => (
            cur_slot.get_cycle(grpc.grpc_config.periods_per_cycle),
            cur_slot,
        ),
        Ok(None) => (0, Slot::new(0, 0)),
        Err(e) => return Err(GrpcError::ModelsError(e)),
    };

    // Get the list of stakers, filtered by the specified minimum and maximum roll counts.
    let mut staker_vec = grpc
        .execution_controller
        .get_cycle_active_rolls(cur_cycle)
        .into_iter()
        .filter_map(|(addr, rolls)| {
            if let Some(min_rolls) = filter_opt.0 {
                if rolls < min_rolls {
                    return None;
                }
            }
            if let Some(max_rolls) = filter_opt.1 {
                if rolls > max_rolls {
                    return None;
                }
            }
            Some((addr.to_string(), rolls))
        })
        .collect::<Vec<(String, u64)>>();

    // Sort the stakers by their roll counts in descending order.
    staker_vec.sort_by_key(|&(_, roll_counts)| std::cmp::Reverse(roll_counts));

    if let Some(limit) = filter_opt.2 {
        staker_vec = staker_vec
            .into_iter()
            .take(limit as usize)
            .collect::<Vec<(String, u64)>>();
    }

    let stakers = staker_vec
        .into_iter()
        .map(|(address, rolls)| grpc_model::StakerEntry { address, rolls })
        .collect();

    Ok(grpc_api::GetStakersResponse { stakers })
}

#[cfg(feature = "execution-trace")]
/// recursive function to convert a AbiTrace struct
pub fn into_element(abi_trace: &AbiTrace) -> AbiCallStackElementParent {
    if abi_trace.sub_calls.is_none() {
        AbiCallStackElementParent {
            call_stack_element: Some(CallStackElement::Element(AbiCallStackElement {
                name: abi_trace.name.clone(),
                parameters: abi_trace
                    .parameters
                    .iter()
                    .map(|p| serde_json::to_string(p).unwrap_or_default())
                    .collect::<Vec<String>>(),
                return_value: serde_json::to_string(&abi_trace.return_value).unwrap_or_default(),
            })),
        }
    } else {
        AbiCallStackElementParent {
            call_stack_element: Some(CallStackElement::ElementCall(AbiCallStackElementCall {
                name: abi_trace.name.clone(),
                parameters: abi_trace
                    .parameters
                    .iter()
                    .map(|p| serde_json::to_string(p).unwrap_or_default())
                    .collect(),
                return_value: serde_json::to_string(&abi_trace.return_value).unwrap_or_default(),
                sub_calls: abi_trace
                    .sub_calls
                    .clone()
                    .unwrap_or_default()
                    .iter()
                    .map(into_element)
                    .collect(),
            })),
        }
    }
}

#[cfg(feature = "execution-trace")]
/// Get slot transfers
pub(crate) fn get_slot_transfers(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetSlotTransfersRequest>,
) -> Result<grpc_api::GetSlotTransfersResponse, GrpcError> {
    use massa_proto_rs::massa::api::v1::GetSlotTransfersResponse;

    let slots = request.into_inner().slots;

    let mut transfer_each_slot: Vec<TransferInfos> = vec![];
    for slot in slots {
        let mut slot_transfers = TransferInfos {
            slot: slot.clone().into(),
            transfers: vec![],
        };
        let abi_calls = grpc
            .execution_controller
            .get_slot_abi_call_stack(slot.clone().into());
        if let Some(abi_calls) = abi_calls {
            // flatten & filter transfer trace in asc_call_stacks

            let abi_transfer_1 = "assembly_script_transfer_coins".to_string();
            let abi_transfer_2 = "assembly_script_transfer_coins_for".to_string();
            let abi_transfer_3 = "abi_transfer_coins".to_string();
            let transfer_abi_names = vec![abi_transfer_1, abi_transfer_2, abi_transfer_3];
            for (i, asc_call_stack) in abi_calls.asc_call_stacks.iter().enumerate() {
                for abi_trace in asc_call_stack {
                    let only_transfer = abi_trace.flatten_filter(&transfer_abi_names);
                    for transfer in only_transfer {
                        let (t_from, t_to, t_amount) = transfer.parse_transfer();
                        slot_transfers.transfers.push(TransferInfo {
                            from: t_from.clone(),
                            to: t_to.clone(),
                            amount: t_amount,
                            operation_id_or_asc_index: Some(
                                grpc_api::transfer_info::OperationIdOrAscIndex::AscIndex(i as u64),
                            ),
                        });
                    }
                }
            }

            for op_call_stack in abi_calls.operation_call_stacks {
                let op_id = op_call_stack.0;
                let op_call_stack = op_call_stack.1;
                for abi_trace in op_call_stack {
                    let only_transfer = abi_trace.flatten_filter(&transfer_abi_names);
                    for transfer in only_transfer {
                        let (t_from, t_to, t_amount) = transfer.parse_transfer();
                        slot_transfers.transfers.push(TransferInfo {
                            from: t_from.clone(),
                            to: t_to.clone(),
                            amount: t_amount,
                            operation_id_or_asc_index: Some(
                                grpc_api::transfer_info::OperationIdOrAscIndex::OperationId(
                                    op_id.to_string(),
                                ),
                            ),
                        });
                    }
                }
            }
        }

        let transfers = grpc
            .execution_controller
            .get_transfers_for_slot(slot.into());
        if let Some(transfers) = transfers {
            for transfer in transfers {
                slot_transfers.transfers.push(TransferInfo {
                    from: transfer.from.to_string(),
                    to: transfer.to.to_string(),
                    amount: transfer.amount.to_raw(),
                    operation_id_or_asc_index: Some(
                        grpc_api::transfer_info::OperationIdOrAscIndex::OperationId(
                            transfer.op_id.to_string(),
                        ),
                    ),
                });
            }
        }

        transfer_each_slot.push(slot_transfers);
    }

    Ok(GetSlotTransfersResponse { transfer_each_slot })
}

#[cfg(feature = "execution-trace")]
/// Get operation ABI call stacks
pub(crate) fn get_operation_abi_call_stacks(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetOperationAbiCallStacksRequest>,
) -> Result<grpc_api::GetOperationAbiCallStacksResponse, GrpcError> {
    let op_ids_ = request.into_inner().operation_ids;

    let op_ids: Vec<OperationId> = op_ids_
        .iter()
        .map(|o| OperationId::from_str(o))
        .collect::<Result<Vec<_>, _>>()?;

    let mut elements = vec![];
    for op_id in op_ids {
        let abi_traces_ = grpc
            .execution_controller
            .get_operation_abi_call_stack(op_id);
        if let Some(abi_traces) = abi_traces_ {
            for abi_trace in abi_traces.iter() {
                elements.push(into_element(abi_trace));
            }
        } else {
            elements.push(AbiCallStackElementParent {
                call_stack_element: None,
            })
        }
    }

    let resp = GetOperationAbiCallStacksResponse {
        call_stacks: vec![AbiCallStack {
            call_stack: elements,
        }],
    };

    Ok(resp)
}

#[cfg(feature = "execution-trace")]
pub(crate) fn get_slot_abi_call_stacks(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetSlotAbiCallStacksRequest>,
) -> Result<grpc_api::GetSlotAbiCallStacksResponse, GrpcError> {
    let slots = request.into_inner().slots;

    let mut slot_elements = vec![];
    for slot in slots {
        let call_stack_ = grpc
            .execution_controller
            .get_slot_abi_call_stack(slot.into());

        let mut slot_abi_call_stacks = SlotAbiCallStacks {
            asc_call_stacks: vec![],
            operation_call_stacks: vec![],
        };

        if let Some(call_stack) = call_stack_ {
            for (i, asc_call_stack) in call_stack.asc_call_stacks.into_iter().enumerate() {
                slot_abi_call_stacks.asc_call_stacks.push(AscabiCallStack {
                    index: i as u64,
                    call_stack: asc_call_stack.iter().map(into_element).collect(),
                })
            }
            for (op_id, op_call_stack) in call_stack.operation_call_stacks {
                slot_abi_call_stacks
                    .operation_call_stacks
                    .push(OperationAbiCallStack {
                        operation_id: op_id.to_string(),
                        call_stack: op_call_stack.iter().map(into_element).collect(),
                    })
            }
        }
        slot_elements.push(slot_abi_call_stacks);
    }

    let resp = GetSlotAbiCallStacksResponse {
        slot_call_stacks: slot_elements,
    };

    Ok(resp)
}

/// Get next block best parents
pub(crate) fn get_next_block_best_parents(
    grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::GetNextBlockBestParentsRequest>,
) -> Result<grpc_api::GetNextBlockBestParentsResponse, GrpcError> {
    let block_parents = grpc
        .consensus_controller
        .get_best_parents()
        .into_iter()
        .map(|p| grpc_model::BlockParent {
            block_id: p.0.to_string(),
            period: p.1,
        })
        .collect();
    Ok(grpc_api::GetNextBlockBestParentsResponse { block_parents })
}

/// Get operations
pub(crate) fn get_operations(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetOperationsRequest>,
) -> Result<grpc_api::GetOperationsResponse, GrpcError> {
    let operation_ids = request.into_inner().operation_ids;

    if operation_ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no operations ids specified".to_string(),
        ));
    }

    if operation_ids.len() as u32 > grpc.grpc_config.max_operation_ids_per_request {
        return Err(GrpcError::InvalidArgument(format!("too many operations received. Only a maximum of {} operations are accepted per request", grpc.grpc_config.max_operation_ids_per_request)));
    }

    let operation_ids: Vec<OperationId> = operation_ids
        .into_iter()
        .take(grpc.grpc_config.max_operation_ids_per_request as usize + 1)
        .map(|id| {
            OperationId::from_str(id.as_str())
                .map_err(|_| GrpcError::InvalidArgument(format!("invalid operation id: {}", id)))
        })
        .collect::<Result<_, _>>()?;

    let secure_share_operations: Vec<SecureShareOperation> = {
        let read_ops = grpc.storage.read_operations();
        operation_ids
            .iter()
            .filter_map(|id| read_ops.get(id).cloned())
            .collect()
    };

    let storage_info: Vec<(SecureShareOperation, PreHashSet<BlockId>)> = {
        let read_blocks = grpc.storage.read_blocks();
        secure_share_operations
            .into_iter()
            .map(|secure_share_operation| {
                let op_id = secure_share_operation.id;
                (
                    secure_share_operation,
                    read_blocks
                        .get_blocks_by_operation(&op_id)
                        .cloned()
                        .unwrap_or_default(),
                )
            })
            .collect()
    };

    let operations: Vec<grpc_model::OperationWrapper> = storage_info
        .into_iter()
        .map(|secure_share| {
            let (secure_share_operation, block_ids) = secure_share;
            grpc_model::OperationWrapper {
                thread: secure_share_operation
                    .content_creator_address
                    .get_thread(grpc.grpc_config.thread_count) as u32,
                operation: Some(secure_share_operation.into()),
                block_ids: block_ids.into_iter().map(|id| id.to_string()).collect(),
            }
        })
        .collect();

    Ok(grpc_api::GetOperationsResponse {
        wrapped_operations: operations,
    })
}

/// Get smart contract execution events
pub(crate) fn get_sc_execution_events(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetScExecutionEventsRequest>,
) -> Result<grpc_api::GetScExecutionEventsResponse, GrpcError> {
    let event_filter = to_event_filter(request.into_inner().filters)?;
    let events: Vec<grpc_model::ScExecutionEvent> = grpc
        .execution_controller
        .get_filtered_sc_output_event(event_filter)
        .into_iter()
        .map(|event| event.into())
        .collect();

    Ok(grpc_api::GetScExecutionEventsResponse { events })
}

//  Get selector draws
pub(crate) fn get_selector_draws(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetSelectorDrawsRequest>,
) -> Result<grpc_api::GetSelectorDrawsResponse, GrpcError> {
    let inner_req = request.into_inner();
    if inner_req.filters.len() as u32 > grpc.grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc.grpc_config.max_filters_per_request
        )));
    }

    let mut addresses_filter: Option<PreHashSet<Address>> = None;
    let mut slot_ranges_filter: Option<HashSet<SlotRange>> = None;
    // Get params filter from the request.
    for query in inner_req.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::selector_draws_filter::Filter::Addresses(addrs) => {
                    if addrs.addresses.len() as u32 > grpc.grpc_config.max_addresses_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many addresses received. Only a maximum of {} addresses are accepted per request",
                            grpc.grpc_config.max_addresses_per_request
                        )));
                    }
                    let addresses = addresses_filter.get_or_insert_with(PreHashSet::default);
                    for address in addrs.addresses {
                        addresses.insert(Address::from_str(&address).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid address: {}", address))
                        })?);
                    }
                }
                grpc_api::selector_draws_filter::Filter::SlotRange(s_range) => {
                    let slot_ranges = slot_ranges_filter.get_or_insert_with(HashSet::new);
                    if slot_ranges.len() as u32 > grpc.grpc_config.max_slot_ranges_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many slot ranges received. Only a maximum of {} slot ranges are accepted per request",
                            grpc.grpc_config.max_slot_ranges_per_request
                        )));
                    }

                    let start_slot: Option<Slot> = s_range.start_slot.map(|s| s.into());
                    let end_slot: Option<Slot> = s_range.end_slot.map(|s| s.into());

                    let slot_range = SlotRange {
                        start_slot,
                        end_slot,
                    };
                    slot_range.check()?;
                    slot_ranges.insert(slot_range);
                }
            }
        }
    }

    // filter by slot ranges
    let selection_draws: HashSet<SlotDraw> = if let Some(slot_ranges) = slot_ranges_filter {
        if slot_ranges.is_empty() {
            return Err(GrpcError::InvalidArgument(
                "at least, one slot range is required".to_string(),
            ));
        }

        let mut start_slot = Slot::new(0, 0); // inclusive
        let mut end_slot = Slot::new(u64::MAX, grpc.grpc_config.thread_count - 1); // exclusive
        for slot_range in &slot_ranges {
            start_slot = start_slot.max(slot_range.start_slot.unwrap_or_else(|| Slot::new(0, 0)));
            end_slot = end_slot.min(
                slot_range
                    .end_slot
                    .unwrap_or_else(|| Slot::new(u64::MAX, grpc.grpc_config.thread_count - 1)),
            );
        }
        end_slot = end_slot.max(start_slot);

        // get future draws from selector
        let mut restrict_to_addresses: Option<&PreHashSet<Address>> = None;
        if let Some(addresses) = &addresses_filter {
            if !addresses.is_empty() {
                restrict_to_addresses = Some(addresses);
            }
        }

        grpc.selector_controller
            .get_available_selections_in_range(start_slot..=end_slot, restrict_to_addresses)
            .unwrap_or_default()
            .into_iter()
            .map(|(v_slot, v_sel)| {
                let endorsement_producers: Vec<EndorsementDraw> = v_sel
                    .endorsements
                    .into_iter()
                    .enumerate()
                    .map(|(index, endo_sel)| EndorsementDraw {
                        index: index as u64,
                        producer: endo_sel.to_string(),
                    })
                    .collect();

                SlotDraw {
                    slot: Some(v_slot),
                    block_producer: Some(v_sel.producer.to_string()),
                    endorsement_draws: endorsement_producers,
                }
            })
            .collect()
    } else {
        return Err(GrpcError::InvalidArgument(
            "at least, one slot range is required".to_string(),
        ));
    };

    Ok(grpc_api::GetSelectorDrawsResponse {
        draws: selection_draws.into_iter().map(Into::into).collect(),
    })
}

//  Get status
pub(crate) fn get_status(
    grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::GetStatusRequest>,
) -> Result<grpc_api::GetStatusResponse, GrpcError> {
    let config = CompactConfig::default();
    let now = MassaTime::now();
    let last_slot = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    )?;

    let current_cycle = last_slot
        .unwrap_or_else(|| Slot::new(0, 0))
        .get_cycle(grpc.grpc_config.periods_per_cycle);
    let cycle_duration = grpc
        .grpc_config
        .t0
        .checked_mul(grpc.grpc_config.periods_per_cycle)?;
    let current_cycle_time = if current_cycle == 0 {
        grpc.grpc_config.genesis_timestamp
    } else {
        cycle_duration
            .checked_mul(current_cycle)
            .and_then(|elapsed_time_before_current_cycle| {
                grpc.grpc_config
                    .genesis_timestamp
                    .checked_add(elapsed_time_before_current_cycle)
            })?
    };
    let next_cycle_time = current_cycle_time.checked_add(cycle_duration)?;
    //TODO to be enhanced
    let empty_request = ExecutionQueryRequest { requests: vec![] };
    let state = grpc.execution_controller.query_state(empty_request);

    let status = grpc_model::PublicStatus {
        node_id: grpc.node_id.to_string(),
        version: grpc.version.to_string(),
        current_time: Some(now.into()),
        current_cycle,
        current_cycle_time: Some(current_cycle_time.into()),
        next_cycle_time: Some(next_cycle_time.into()),
        last_executed_final_slot: Some(state.final_cursor.into()),
        last_executed_speculative_slot: Some(state.candidate_cursor.into()),
        final_state_fingerprint: state.final_state_fingerprint.to_string(),
        config: Some(config.into()),
        chain_id: grpc.grpc_config.chain_id,
        minimal_fees: Some(grpc.grpc_config.minimal_fees.into()),
    };

    Ok(grpc_api::GetStatusResponse {
        status: Some(status),
    })
}

/// Get transactions throughput
pub(crate) fn get_transactions_throughput(
    grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::GetTransactionsThroughputRequest>,
) -> Result<grpc_api::GetTransactionsThroughputResponse, GrpcError> {
    let stats = grpc.execution_controller.get_stats();
    let nb_sec_range = stats
        .time_window_end
        .saturating_sub(stats.time_window_start)
        .to_duration()
        .as_secs();

    let throughput = stats
        .final_executed_operations_count
        .checked_div(nb_sec_range as usize)
        .unwrap_or_default() as u32;

    Ok(grpc_api::GetTransactionsThroughputResponse { throughput })
}

/// Get query state
pub(crate) fn query_state(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::QueryStateRequest>,
) -> Result<grpc_api::QueryStateResponse, GrpcError> {
    let queries = request
        .into_inner()
        .queries
        .into_iter()
        .map(to_querystate_filter)
        .collect::<Result<Vec<_>, _>>()?;

    if queries.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no query items specified".to_string(),
        ));
    }

    if queries.len() as u32 > grpc.grpc_config.max_query_items_per_request {
        return Err(GrpcError::InvalidArgument(format!("too many query items received. Only a maximum of {} operations are accepted per request", grpc.grpc_config.max_query_items_per_request)));
    }

    let response = grpc
        .execution_controller
        .query_state(ExecutionQueryRequest { requests: queries });

    Ok(grpc_api::QueryStateResponse {
        final_cursor: Some(response.final_cursor.into()),
        candidate_cursor: Some(response.candidate_cursor.into()),
        final_state_fingerprint: response.final_state_fingerprint.to_string(),
        responses: response
            .responses
            .into_iter()
            .map(to_execution_query_response)
            .collect(),
    })
}

/// Search blocks
pub(crate) fn search_blocks(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::SearchBlocksRequest>,
) -> Result<grpc_api::SearchBlocksResponse, GrpcError> {
    let inner_req = request.into_inner();
    if inner_req.filters.len() as u32 > grpc.grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc.grpc_config.max_filters_per_request
        )));
    }

    let mut block_ids_filter: Option<PreHashSet<BlockId>> = None;
    let mut addresses_filter: Option<PreHashSet<Address>> = None;
    let mut slot_ranges_filter: Option<HashSet<SlotRange>> = None;

    // Get params filter from the request.
    for query in inner_req.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::search_blocks_filter::Filter::BlockIds(ids) => {
                    if ids.block_ids.len() as u32 > grpc.grpc_config.max_block_ids_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many block ids received. Only a maximum of {} block ids are accepted per request",
                            grpc.grpc_config.max_block_ids_per_request
                        )));
                    }
                    let block_ids = block_ids_filter.get_or_insert_with(PreHashSet::default);
                    for block_id in ids.block_ids {
                        block_ids.insert(BlockId::from_str(&block_id).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid block id: {}", block_id))
                        })?);
                    }
                }
                grpc_api::search_blocks_filter::Filter::Addresses(addrs) => {
                    if addrs.addresses.len() as u32 > grpc.grpc_config.max_addresses_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many addresses received. Only a maximum of {} addresses are accepted per request",
                            grpc.grpc_config.max_addresses_per_request
                        )));
                    }
                    let addresses = addresses_filter.get_or_insert_with(PreHashSet::default);
                    for address in addrs.addresses {
                        addresses.insert(Address::from_str(&address).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid address: {}", address))
                        })?);
                    }
                }
                grpc_api::search_blocks_filter::Filter::SlotRange(s_range) => {
                    let slot_ranges = slot_ranges_filter.get_or_insert_with(HashSet::new);
                    if slot_ranges.len() as u32 > grpc.grpc_config.max_slot_ranges_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many slot ranges received. Only a maximum of {} slot ranges are accepted per request",
                            grpc.grpc_config.max_slot_ranges_per_request
                        )));
                    }

                    let start_slot: Option<Slot> = s_range.start_slot.map(|s| s.into());
                    let end_slot: Option<Slot> = s_range.end_slot.map(|s| s.into());

                    let slot_range = SlotRange {
                        start_slot,
                        end_slot,
                    };
                    slot_range.check()?;
                    slot_ranges.insert(slot_range);
                }
            }
        }
    }

    // if no filter provided return an error
    if block_ids_filter.is_none() && addresses_filter.is_none() && slot_ranges_filter.is_none() {
        return Err(GrpcError::InvalidArgument("no filter provided".to_string()));
    }

    let mut res: Option<PreHashSet<BlockId>> = None;

    // filter by block ids
    if let Some(mut b_ids) = block_ids_filter {
        let read_lock = grpc.storage.read_blocks();
        b_ids.retain(|id: &BlockId| read_lock.contains(id));

        res = Some(b_ids);
    }

    // filter by addresses
    if let Some(addrs) = addresses_filter {
        let b_ids: PreHashSet<BlockId> = {
            let read_lock = grpc.storage.read_blocks();
            let mut b_ids: PreHashSet<BlockId> = PreHashSet::default();
            for addr in addrs {
                if let Some(addr_b_ids) = read_lock.get_blocks_created_by(&addr) {
                    b_ids.extend(addr_b_ids.clone());
                }
            }

            b_ids
        };
        if let Some(block_ids) = res.as_mut() {
            block_ids.retain(|id: &BlockId| b_ids.contains(id));
        } else {
            res = Some(b_ids)
        }
    }

    // filter by slot ranges
    if let Some(slot_ranges) = slot_ranges_filter {
        let mut start_slot = Slot::new(0, 0); // inclusive
        let mut end_slot = Slot::new(u64::MAX, grpc.grpc_config.thread_count - 1); // exclusive
        for slot_range in &slot_ranges {
            start_slot = start_slot.max(slot_range.start_slot.unwrap_or_else(|| Slot::new(0, 0)));
            end_slot = end_slot.min(
                slot_range
                    .end_slot
                    .unwrap_or_else(|| Slot::new(u64::MAX, grpc.grpc_config.thread_count - 1)),
            );
        }
        end_slot = end_slot.max(start_slot);

        let read_lock = grpc.storage.read_blocks();
        let b_ids: PreHashSet<BlockId> =
            read_lock.aggregate_blocks_by_slot_range(start_slot..end_slot);

        if let Some(block_ids) = res.as_mut() {
            block_ids.retain(|id: &BlockId| b_ids.contains(id));
        } else {
            res = Some(b_ids)
        }
    }

    let block_ids: Vec<BlockId> = res.unwrap_or_default().into_iter().collect();

    if block_ids.is_empty() {
        return Ok(grpc_api::SearchBlocksResponse {
            block_infos: vec![],
        });
    }

    let blocks_status = grpc.consensus_controller.get_block_statuses(&block_ids);

    let result = block_ids
        .iter()
        .zip(blocks_status)
        .map(|(block_id, block_graph_status)| grpc_model::BlockInfo {
            block_id: block_id.to_string(),
            status: block_graph_status.into(),
        })
        .collect();

    Ok(grpc_api::SearchBlocksResponse {
        block_infos: result,
    })
}

/// Search endorsements
pub(crate) fn search_endorsements(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::SearchEndorsementsRequest>,
) -> Result<grpc_api::SearchEndorsementsResponse, GrpcError> {
    let inner_req = request.into_inner();
    if inner_req.filters.len() as u32 > grpc.grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc.grpc_config.max_filters_per_request
        )));
    }

    let mut endorsement_ids_filter: Option<PreHashSet<EndorsementId>> = None;
    let mut addresses_filter: Option<PreHashSet<Address>> = None;
    let mut block_ids_filter: Option<PreHashSet<BlockId>> = None;

    // Get params filter from the request.
    for query in inner_req.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::search_endorsements_filter::Filter::EndorsementIds(ids) => {
                    if ids.endorsement_ids.len() as u32
                        > grpc.grpc_config.max_endorsement_ids_per_request
                    {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many endorsement ids received. Only a maximum of {} endorsement ids are accepted per request",
                            grpc.grpc_config.max_endorsement_ids_per_request
                        )));
                    }
                    let endorsement_ids =
                        endorsement_ids_filter.get_or_insert_with(PreHashSet::default);
                    for id in ids.endorsement_ids {
                        endorsement_ids.insert(EndorsementId::from_str(&id).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid endorsement id: {}", id))
                        })?);
                    }
                }
                grpc_api::search_endorsements_filter::Filter::Addresses(addrs) => {
                    if addrs.addresses.len() as u32 > grpc.grpc_config.max_addresses_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many addresses received. Only a maximum of {} addresses are accepted per request",
                            grpc.grpc_config.max_addresses_per_request
                        )));
                    }
                    let addresses = addresses_filter.get_or_insert_with(PreHashSet::default);
                    for address in addrs.addresses {
                        addresses.insert(Address::from_str(&address).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid address: {}", address))
                        })?);
                    }
                }
                grpc_api::search_endorsements_filter::Filter::BlockIds(ids) => {
                    if ids.block_ids.len() as u32 > grpc.grpc_config.max_block_ids_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many block ids received. Only a maximum of {} block ids are accepted per request",
                            grpc.grpc_config.max_block_ids_per_request
                        )));
                    }
                    let block_ids = block_ids_filter.get_or_insert_with(PreHashSet::default);
                    for block_id in ids.block_ids {
                        block_ids.insert(BlockId::from_str(&block_id).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid block id: {}", block_id))
                        })?);
                    }
                }
            }
        }
    }

    // if no filter provided return an error
    if endorsement_ids_filter.is_none() && addresses_filter.is_none() && block_ids_filter.is_none()
    {
        return Err(GrpcError::InvalidArgument("no filter provided".to_string()));
    }

    let mut eds_ids: Option<PreHashSet<EndorsementId>> = None;

    // filter by endorsement ids
    if let Some(mut e_ids) = endorsement_ids_filter {
        let read_lock = grpc.storage.read_endorsements();
        e_ids.retain(|id: &EndorsementId| read_lock.contains(id));
        eds_ids = Some(e_ids);
    }

    // filter by addresses
    if let Some(addrs) = addresses_filter {
        let e_ids: PreHashSet<EndorsementId> = {
            let mut e_ids: PreHashSet<EndorsementId> = PreHashSet::default();
            let read_lock = grpc.storage.read_endorsements();
            for addr in addrs {
                if let Some(addr_e_ids) = read_lock.get_endorsements_created_by(&addr) {
                    e_ids.extend(addr_e_ids.clone());
                }
            }

            e_ids
        };
        if let Some(endorsement_ids) = eds_ids.as_mut() {
            endorsement_ids.retain(|id: &EndorsementId| e_ids.contains(id));
        } else {
            eds_ids = Some(e_ids)
        }
    }

    // filter by block ids
    if let Some(b_ids) = block_ids_filter {
        let mut e_ids: PreHashSet<EndorsementId> = PreHashSet::default();
        let read_lock = grpc.storage.read_blocks();
        for block_id in b_ids {
            if let Some(wrapped_block) = read_lock.get(&block_id) {
                let b_endorsements: PreHashSet<EndorsementId> = wrapped_block
                    .content
                    .header
                    .content
                    .endorsements
                    .iter()
                    .map(|wrapped_endorsement| wrapped_endorsement.id)
                    .collect();
                e_ids.extend(&b_endorsements);
            }
        }

        if let Some(endorsement_ids) = eds_ids.as_mut() {
            endorsement_ids.retain(|id: &EndorsementId| e_ids.contains(id));
        } else {
            eds_ids = Some(e_ids)
        }
    }

    let storage_info: Vec<(EndorsementId, PreHashSet<BlockId>)> = {
        let read_blocks_lock = grpc.storage.read_blocks();
        if let Some(endorsement_ids) = eds_ids {
            endorsement_ids
                .into_iter()
                .map(|id| {
                    let block_ids = read_blocks_lock
                        .get_blocks_by_endorsement(&id)
                        .cloned()
                        .unwrap_or_default();

                    (id, block_ids)
                })
                .collect()
        } else {
            return Ok(grpc_api::SearchEndorsementsResponse {
                endorsement_infos: Vec::new(),
            });
        }
    };

    // keep only the endorsements found in storage
    let e_ids: Vec<EndorsementId> = storage_info.iter().map(|(ed, _)| *ed).collect();

    // ask pool whether it carries the endorsements
    let in_pool = grpc.pool_controller.contains_endorsements(&e_ids);

    // check finality by cross-referencing Consensus and looking for final blocks that contain the endorsement
    let is_final: Vec<bool> = {
        let involved_blocks: Vec<BlockId> = storage_info
            .iter()
            .flat_map(|(_ed, bs)| bs.iter())
            .unique()
            .cloned()
            .collect();

        let involved_block_statuses = grpc
            .consensus_controller
            .get_block_statuses(&involved_blocks);

        let block_statuses: PreHashMap<BlockId, BlockGraphStatus> = involved_blocks
            .into_iter()
            .zip(involved_block_statuses)
            .collect();
        storage_info
            .iter()
            .map(|(_ed, bs)| {
                bs.iter()
                    .any(|b| block_statuses.get(b) == Some(&BlockGraphStatus::Final))
            })
            .collect()
    };

    // gather all values into a vector of EndorsementInfo instances
    let mut res: Vec<grpc_model::EndorsementInfo> = Vec::with_capacity(e_ids.len());
    let zipped_iterator = izip!(
        storage_info.into_iter(),
        in_pool.into_iter(),
        is_final.into_iter()
    );

    for ((e_id, in_blocks), in_pool, is_final) in zipped_iterator {
        res.push(grpc_model::EndorsementInfo {
            in_pool,
            is_final,
            in_blocks: in_blocks
                .into_iter()
                .map(|block_id| block_id.to_string())
                .collect(),
            endorsement_id: e_id.to_string(),
        });
    }

    Ok(grpc_api::SearchEndorsementsResponse {
        endorsement_infos: res,
    })
}

/// Search operations
pub(crate) fn search_operations(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::SearchOperationsRequest>,
) -> Result<grpc_api::SearchOperationsResponse, GrpcError> {
    let inner_req: grpc_api::SearchOperationsRequest = request.into_inner();
    if inner_req.filters.len() as u32 > grpc.grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc.grpc_config.max_filters_per_request
        )));
    }
    let mut operation_ids_filter: Option<PreHashSet<OperationId>> = None;
    let mut addresses_filter: Option<PreHashSet<Address>> = None;

    // Get params filter from the request.
    for query in inner_req.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::search_operations_filter::Filter::OperationIds(ids) => {
                    if ids.operation_ids.len() as u32
                        > grpc.grpc_config.max_operation_ids_per_request
                    {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many operation ids received. Only a maximum of {} operation ids are accepted per request",
                            grpc.grpc_config.max_block_ids_per_request
                        )));
                    }
                    let operation_ids =
                        operation_ids_filter.get_or_insert_with(PreHashSet::default);
                    for id in ids.operation_ids {
                        operation_ids.insert(OperationId::from_str(&id).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid operation id: {}", id))
                        })?);
                    }
                }
                grpc_api::search_operations_filter::Filter::Addresses(addrs) => {
                    if addrs.addresses.len() as u32 > grpc.grpc_config.max_addresses_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many addresses received. Only a maximum of {} addresses are accepted per request",
                            grpc.grpc_config.max_addresses_per_request
                        )));
                    }
                    let addresses = addresses_filter.get_or_insert_with(PreHashSet::default);
                    for address in addrs.addresses {
                        addresses.insert(Address::from_str(&address).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid address: {}", address))
                        })?);
                    }
                }
            }
        }
    }

    if operation_ids_filter.is_none() && addresses_filter.is_none() {
        return Err(GrpcError::InvalidArgument("no filter provided".to_string()));
    }

    let mut ops_ids: Option<PreHashSet<OperationId>> = None;

    // filter by operation ids
    if let Some(mut o_ids) = operation_ids_filter {
        let read_lock = grpc.storage.read_operations();
        o_ids.retain(|id: &OperationId| read_lock.contains(id));
        ops_ids = Some(o_ids);
    }

    // filter by addresses
    if let Some(addrs) = addresses_filter {
        let o_ids: PreHashSet<OperationId> = {
            let read_lock = grpc.storage.read_operations();
            let mut o_ids: PreHashSet<OperationId> = PreHashSet::default();
            for addr in addrs {
                if let Some(addr_o_ids) = read_lock.get_operations_created_by(&addr) {
                    o_ids.extend(addr_o_ids.clone());
                }
            }

            o_ids
        };
        if let Some(operation_ids) = ops_ids.as_mut() {
            operation_ids.retain(|id: &OperationId| o_ids.contains(id));
        } else {
            ops_ids = Some(o_ids)
        }
    }

    let operations: Vec<grpc_model::OperationInfo> = if let Some(operation_ids) = ops_ids {
        let secure_share_operations: Vec<SecureShareOperation> = {
            let read_ops = grpc.storage.read_operations();
            operation_ids
                .iter()
                .filter_map(|id| read_ops.get(id).cloned())
                .collect()
        };

        let storage_info: Vec<(SecureShareOperation, PreHashSet<BlockId>)> = {
            let read_blocks = grpc.storage.read_blocks();
            secure_share_operations
                .into_iter()
                .map(|secure_share_operation| {
                    let op_id = secure_share_operation.id;
                    (
                        secure_share_operation,
                        read_blocks
                            .get_blocks_by_operation(&op_id)
                            .cloned()
                            .unwrap_or_default(),
                    )
                })
                .collect()
        };

        storage_info
            .into_iter()
            .map(|secureshare| {
                let (secureshare_operation, block_ids) = secureshare;
                grpc_model::OperationInfo {
                    id: secureshare_operation.id.to_string(),
                    thread: secureshare_operation
                        .content_creator_address
                        .get_thread(grpc.grpc_config.thread_count)
                        as u32,
                    block_ids: block_ids.into_iter().map(|id| id.to_string()).collect(),
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(grpc_api::SearchOperationsResponse {
        operation_infos: operations,
    })
}
