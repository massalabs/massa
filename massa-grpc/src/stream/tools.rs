use std::collections::HashSet;
use std::str::FromStr;

use crate::SlotRange;
use crate::{config::GrpcConfig, error::GrpcError};
use massa_execution_exports::{ExecutionOutput, SlotExecutionOutput};
use massa_models::address::Address;
use massa_models::block::SecureShareBlock;
use massa_models::block_id::BlockId;
use massa_models::slot::Slot;
use massa_proto_rs::massa::api::v1::{self as grpc_api, NewSlotExecutionOutputsRequest};
use massa_proto_rs::massa::model::v1::{self as grpc_model};

/// Type declaration for NewSlotExecutionOutputsFilter
#[derive(Clone, Debug, Default)]
pub(crate) struct FilterNewSlotExec {
    // Execution output status to filter
    status_filter: Option<i32>,
    // Slot range to filter
    slot_ranges_filter: Option<Vec<grpc_model::SlotRange>>,
    // Async pool changes filter
    async_pool_changes_filter: Option<Vec<grpc_api::async_pool_changes_filter::Filter>>,
    // Executed denounciation filter
    executed_denounciation_filter: Option<grpc_api::executed_denounciation_filter::Filter>,
    // Execution event filter
    execution_event_filter: Option<Vec<grpc_api::execution_event_filter::Filter>>,
    // Executed ops changes filter
    executed_ops_changes_filter: Option<Vec<grpc_api::executed_ops_changes_filter::Filter>>,
    // Ledger changes filter
    ledger_changes_filter: Option<Vec<grpc_api::ledger_changes_filter::Filter>>,
}

// This function returns a filter from the request
pub(crate) fn get_filter_slot_exec_out(
    request: NewSlotExecutionOutputsRequest,
    grpc_config: &GrpcConfig,
) -> Result<FilterNewSlotExec, GrpcError> {
    if request.filters.len() as u32 > grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc_config.max_filters_per_request
        )));
    }

    let mut result = FilterNewSlotExec::default();

    for query in request.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::new_slot_execution_outputs_filter::Filter::Status(status) => result.status_filter = Some(status),
                grpc_api::new_slot_execution_outputs_filter::Filter::SlotRange(s_range) => {
                        result.slot_ranges_filter.get_or_insert(Vec::new()).push(s_range);
                },
                grpc_api::new_slot_execution_outputs_filter::Filter::AsyncPoolChangesFilter(filter) => {
                    if let Some(request_f) = filter.filter {
                        result.async_pool_changes_filter.get_or_insert(Vec::new()).push(request_f);
                    }
                },
                grpc_api::new_slot_execution_outputs_filter::Filter::ExecutedDenounciationFilter(filter) => result.executed_denounciation_filter = filter.filter,
                grpc_api::new_slot_execution_outputs_filter::Filter::EventFilter(filter) => {
                    if let Some(request_f) = filter.filter {
                        result.execution_event_filter.get_or_insert(Vec::new()).push(request_f);
                    }
                },
                grpc_api::new_slot_execution_outputs_filter::Filter::ExecutedOpsChangesFilter(filter) => {
                    if let Some(request_f) = filter.filter {
                        result.executed_ops_changes_filter.get_or_insert(Vec::new()).push(request_f);
                    }
                },
                grpc_api::new_slot_execution_outputs_filter::Filter::LedgerChangesFilter(filter) => {
                    if let Some(request_f) = filter.filter {
                        result.ledger_changes_filter.get_or_insert(Vec::new()).push(request_f);
                    }
                },
            }
        }
    }

    Ok(result)
}

/// Return if the slot execution outputs should be send to client
pub(crate) fn filter_map(
    slot_execution_output: SlotExecutionOutput,
    filters: &FilterNewSlotExec,
    grpc_config: &GrpcConfig,
) -> Option<SlotExecutionOutput> {
    match slot_execution_output {
        SlotExecutionOutput::ExecutedSlot(e_output) => filter_map_exec_output_inner(
            e_output,
            filters,
            grpc_config,
            grpc_model::ExecutionOutputStatus::Candidate as i32,
        )
        .map(SlotExecutionOutput::ExecutedSlot),
        SlotExecutionOutput::FinalizedSlot(e_output) => filter_map_exec_output_inner(
            e_output,
            filters,
            grpc_config,
            grpc_model::ExecutionOutputStatus::Final as i32,
        )
        .map(SlotExecutionOutput::FinalizedSlot),
    }
}

// Return if the execution outputs should be send and remove the fields that are not needed
fn filter_map_exec_output_inner(
    mut exec_output: ExecutionOutput,
    filters: &FilterNewSlotExec,
    _grpc_config: &GrpcConfig,
    exec_status: i32,
) -> Option<ExecutionOutput> {
    // Filter on status
    if let Some(status) = filters.status_filter {
        if status.ne(&exec_status) {
            return None;
        }
    }

    // Filter Slot Range
    if let Some(slot_ranges) = &filters.slot_ranges_filter {
        if slot_ranges.iter().any(|slot_range| {
            slot_range
                .start_slot
                .map_or(false, |start| exec_output.slot < start.into())
                || slot_range
                    .end_slot
                    .map_or(false, |end| exec_output.slot >= end.into())
        }) {
            return None;
        }
    }

    // Filter Exec event
    if let Some(execution_event_filter) = filters.execution_event_filter.as_ref() {
        exec_output.events.0.retain(|event| {
            execution_event_filter.iter().all(|filter| match filter {
                grpc_api::execution_event_filter::Filter::None(_) => false,
                grpc_api::execution_event_filter::Filter::CallerAddress(addr) => event
                    .context
                    .call_stack
                    .front()
                    .map_or(false, |call| call.to_string().eq(addr)),
                grpc_api::execution_event_filter::Filter::EmitterAddress(addr) => event
                    .context
                    .call_stack
                    .back()
                    .map_or(false, |emit| emit.to_string().eq(addr)),
                grpc_api::execution_event_filter::Filter::OriginalOperationId(ope_id) => event
                    .context
                    .origin_operation_id
                    .map_or(false, |ope| ope.to_string().eq(ope_id)),
                grpc_api::execution_event_filter::Filter::IsFailure(b) => {
                    event.context.is_error.eq(b)
                }
            })
        });

        if exec_output.events.0.is_empty() {
            return None;
        }
    }

    // Filter async pool changes
    if let Some(async_pool_changes_filter) = &filters.async_pool_changes_filter {
        exec_output.state_changes.async_pool_changes.0.retain(
            |(_msg_id, _slot, _emission_index), changes| {
                async_pool_changes_filter.iter().all(|filter| match filter {
                    grpc_api::async_pool_changes_filter::Filter::None(_empty) => false,
                    grpc_api::async_pool_changes_filter::Filter::Type(filter_type) => match changes
                    {
                        massa_models::types::SetUpdateOrDelete::Set(_) => {
                            (grpc_model::AsyncPoolChangeType::Set as i32).eq(filter_type)
                        }
                        massa_models::types::SetUpdateOrDelete::Update(_) => {
                            (grpc_model::AsyncPoolChangeType::Update as i32).eq(filter_type)
                        }
                        massa_models::types::SetUpdateOrDelete::Delete => {
                            (grpc_model::AsyncPoolChangeType::Delete as i32).eq(filter_type)
                        }
                    },
                    grpc_api::async_pool_changes_filter::Filter::Handler(handler) => {
                        match changes {
                            massa_models::types::SetUpdateOrDelete::Set(msg) => {
                                msg.function.eq(handler)
                            }
                            massa_models::types::SetUpdateOrDelete::Update(msg) => {
                                match &msg.function {
                                    massa_models::types::SetOrKeep::Set(func) => func.eq(handler),
                                    massa_models::types::SetOrKeep::Keep => false,
                                }
                            }
                            massa_models::types::SetUpdateOrDelete::Delete => false,
                        }
                    }
                    grpc_api::async_pool_changes_filter::Filter::DestinationAddress(
                        filter_dest_addr,
                    ) => match changes {
                        massa_models::types::SetUpdateOrDelete::Set(msg) => {
                            msg.destination.to_string().eq(filter_dest_addr)
                        }
                        massa_models::types::SetUpdateOrDelete::Update(msg) => {
                            match msg.destination {
                                massa_models::types::SetOrKeep::Set(dest) => {
                                    dest.to_string().eq(filter_dest_addr)
                                }
                                massa_models::types::SetOrKeep::Keep => false,
                            }
                        }
                        massa_models::types::SetUpdateOrDelete::Delete => false,
                    },
                    grpc_api::async_pool_changes_filter::Filter::EmitterAddress(
                        filter_emit_addr,
                    ) => match changes {
                        massa_models::types::SetUpdateOrDelete::Set(msg) => {
                            msg.sender.to_string().eq(filter_emit_addr)
                        }
                        massa_models::types::SetUpdateOrDelete::Update(msg) => match msg.sender {
                            massa_models::types::SetOrKeep::Set(addr) => {
                                addr.to_string().eq(filter_emit_addr)
                            }
                            massa_models::types::SetOrKeep::Keep => false,
                        },
                        massa_models::types::SetUpdateOrDelete::Delete => false,
                    },
                    grpc_api::async_pool_changes_filter::Filter::CanBeExecuted(filter_exec) => {
                        match changes {
                            massa_models::types::SetUpdateOrDelete::Set(msg) => {
                                msg.can_be_executed.eq(filter_exec)
                            }
                            massa_models::types::SetUpdateOrDelete::Update(msg) => {
                                match msg.can_be_executed {
                                    massa_models::types::SetOrKeep::Set(b) => b.eq(filter_exec),
                                    massa_models::types::SetOrKeep::Keep => false,
                                }
                            }
                            massa_models::types::SetUpdateOrDelete::Delete => false,
                        }
                    }
                })
            },
        );

        if exec_output.state_changes.async_pool_changes.0.is_empty() {
            return None;
        }
    }

    if let Some(executed_denounciation_filter) = &filters.executed_denounciation_filter {
        match executed_denounciation_filter {
            grpc_api::executed_denounciation_filter::Filter::None(_empty) => {
                exec_output
                    .state_changes
                    .executed_denunciations_changes
                    .clear();
            }
        }
    }

    // Filter executed ops id
    if let Some(executed_ops_changes_filter) = &filters.executed_ops_changes_filter {
        exec_output
            .state_changes
            .executed_ops_changes
            .retain(|op, (_success, _slot)| {
                executed_ops_changes_filter.iter().all(|f| match f {
                    grpc_api::executed_ops_changes_filter::Filter::None(_empty) => false,
                    grpc_api::executed_ops_changes_filter::Filter::OperationId(filter_ope_id) => {
                        op.to_string().eq(filter_ope_id)
                    }
                })
            });

        if exec_output.state_changes.executed_ops_changes.is_empty() {
            return None;
        }
    }

    // Filter ledger changes
    if let Some(ledger_changes_filter) = &filters.ledger_changes_filter {
        exec_output
            .state_changes
            .ledger_changes
            .0
            .retain(|addr, _ledger_change| {
                ledger_changes_filter.iter().all(|filter| match filter {
                    grpc_api::ledger_changes_filter::Filter::None(_empty) => false,
                    grpc_api::ledger_changes_filter::Filter::Address(filter_addr) => {
                        addr.to_string().eq(filter_addr)
                    }
                })
            });

        if exec_output.state_changes.ledger_changes.0.is_empty() {
            return None;
        }
    }

    Some(exec_output)
}

// Type declaration for NewBlocksFilter
#[derive(Clone, Debug)]
pub(crate) struct FilterNewBlocks {
    // Block ids to filter
    block_ids: Option<HashSet<BlockId>>,
    // Addresses to filter
    addresses: Option<HashSet<Address>>,
    // Slot range to filter
    slot_ranges: Option<HashSet<SlotRange>>,
}

// This function returns a filter from the request
pub(crate) fn get_filter_new_blocks(
    request: grpc_api::NewBlocksRequest,
    grpc_config: &GrpcConfig,
) -> Result<FilterNewBlocks, GrpcError> {
    if request.filters.len() as u32 > grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc_config.max_filters_per_request
        )));
    }

    let mut block_ids_filter: Option<HashSet<BlockId>> = None;
    let mut addresses_filter: Option<HashSet<Address>> = None;
    let mut slot_ranges_filter: Option<HashSet<SlotRange>> = None;

    // Get params filter from the request.
    for query in request.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::new_blocks_filter::Filter::BlockIds(ids) => {
                    if ids.block_ids.len() as u32 > grpc_config.max_block_ids_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many block ids received. Only a maximum of {} block ids are accepted per request",
                            grpc_config.max_block_ids_per_request
                        )));
                    }

                    let block_ids = block_ids_filter.get_or_insert_with(HashSet::new);
                    for block_id in ids.block_ids {
                        block_ids.insert(BlockId::from_str(&block_id).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid block id: {}", block_id))
                        })?);
                    }
                }
                grpc_api::new_blocks_filter::Filter::Addresses(addrs) => {
                    if addrs.addresses.len() as u32 > grpc_config.max_addresses_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many addresses received. Only a maximum of {} addresses are accepted per request",
                         grpc_config.max_addresses_per_request
                        )));
                    }

                    let addresses = addresses_filter.get_or_insert_with(HashSet::new);
                    for address in addrs.addresses {
                        addresses.insert(Address::from_str(&address).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid address: {}", address))
                        })?);
                    }
                }
                grpc_api::new_blocks_filter::Filter::SlotRange(s_range) => {
                    let slot_ranges = slot_ranges_filter.get_or_insert_with(HashSet::new);
                    if slot_ranges.len() as u32 > grpc_config.max_slot_ranges_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many slot ranges received. Only a maximum of {} slot ranges are accepted per request",
                         grpc_config.max_slot_ranges_per_request
                        )));
                    }

                    let start_slot = s_range.start_slot.map(|s| s.into());
                    let end_slot = s_range.end_slot.map(|s| s.into());

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

    Ok(FilterNewBlocks {
        block_ids: block_ids_filter,
        addresses: addresses_filter,
        slot_ranges: slot_ranges_filter,
    })
}

// This function checks if the block should be sent
pub(crate) fn should_send_new_blocks(
    signed_block: &SecureShareBlock,
    filters: &FilterNewBlocks,
    grpc_config: &GrpcConfig,
) -> bool {
    if let Some(block_ids) = &filters.block_ids {
        if !block_ids.contains(&signed_block.id) {
            return false;
        }
    }

    if let Some(addresses) = &filters.addresses {
        if !addresses.contains(&signed_block.content_creator_address) {
            return false;
        }
    }

    if let Some(slot_ranges) = &filters.slot_ranges {
        let mut start_slot = Slot::new(0, 0); // inclusive
        let mut end_slot = Slot::new(u64::MAX, grpc_config.thread_count - 1); // exclusive

        for slot_range in slot_ranges {
            start_slot = start_slot.max(slot_range.start_slot.unwrap_or_else(|| Slot::new(0, 0)));
            end_slot = end_slot.min(
                slot_range
                    .end_slot
                    .unwrap_or_else(|| Slot::new(u64::MAX, grpc_config.thread_count - 1)),
            );
        }
        end_slot = end_slot.max(start_slot);
        let current_slot = signed_block.content.header.content.slot;

        return current_slot >= start_slot // inclusive
            && current_slot < end_slot; // exclusive
    }

    true
}
