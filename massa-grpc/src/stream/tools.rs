use crate::{config::GrpcConfig, error::GrpcError};
use massa_execution_exports::{ExecutionOutput, SlotExecutionOutput};
use massa_proto_rs::massa::api::v1::{self as grpc_api, NewSlotExecutionOutputsRequest};
use massa_proto_rs::massa::model::v1::{self as grpc_model};

/// Type declaration for NewSlotExecutionOutputsFilter
#[derive(Clone, Debug, Default)]
pub(crate) struct Filter {
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
pub(crate) fn get_filter(
    request: NewSlotExecutionOutputsRequest,
    grpc_config: &GrpcConfig,
) -> Result<Filter, GrpcError> {
    if request.filters.len() as u32 > grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc_config.max_filters_per_request
        )));
    }

    let mut result = Filter::default();

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
    filters: &Filter,
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
    filters: &Filter,
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
