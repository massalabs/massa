// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::config::GrpcConfig;
use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_execution_exports::{ExecutionOutput, SlotExecutionOutput};
use massa_proto_rs::massa::api::v1::{self as grpc_api, NewSlotExecutionOutputsRequest};
use massa_proto_rs::massa::model::v1::{self as grpc_model};
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::select;
use tonic::{Request, Streaming};
use tracing::{error, warn};

/// Type declaration for NewSlotExecutionOutputs
pub type NewSlotExecutionOutputsStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::NewSlotExecutionOutputsResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

//TODO implement remaining sub filters
// Type declaration for NewSlotExecutionOutputsFilter
#[derive(Clone, Debug, Default)]
struct Filter {
    // Execution output status to filter
    status_filter: Option<i32>,
    // Slot range to filter
    slot_ranges_filter: Option<grpc_model::SlotRange>,
    // Async pool changes filter
    async_pool_changes_filter: Option<grpc_api::async_pool_changes_filter::Filter>,
    // Executed denounciation filter
    executed_denounciation_filter: Option<grpc_api::executed_denounciation_filter::Filter>,
    // Execution event filter
    execution_event_filter: Option<Vec<grpc_api::execution_event_filter::Filter>>,
    // Executed ops changes filter
    executed_ops_changes_filter: Option<grpc_api::executed_ops_changes_filter::Filter>,
    // Ledger changes filter
    ledger_changes_filter: Option<grpc_api::ledger_changes_filter::Filter>,
}

/// Creates a new stream of new produced and received slot execution outputs
pub(crate) async fn new_slot_execution_outputs(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewSlotExecutionOutputsRequest>>,
) -> Result<NewSlotExecutionOutputsStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new slot execution events channel
    let mut subscriber = grpc
        .execution_channels
        .slot_execution_output_sender
        .subscribe();
    let grpc_config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            let mut filters: Filter = match get_filter(request.clone(), &grpc_config) {
                Ok(filter) => filter,
                Err(err) => {
                    error!("failed to get filter: {}", err);
                    // Send the error response back to the client
                    if let Err(e) = tx.send(Err(err.into())).await {
                        error!("failed to send back NewBlocks error response: {}", e);
                    }
                    return;
                }
            };

            loop {
                select! {
                    // Receive a new slot execution output from the subscriber
                    event = subscriber.recv() => {
                        match event {
                            Ok(massa_slot_execution_output) => {
                                let slot_execution_output = filter_map(massa_slot_execution_output, &filters, &grpc_config);
                                // Check if the slot execution output should be sent
                                if let Some(slot_execution_output) = slot_execution_output {
                                    // Send the new slot execution output through the channel
                                    if let Err(e) = tx.send(Ok(grpc_api::NewSlotExecutionOutputsResponse {
                                            output: Some(slot_execution_output.into())
                                    })).await {
                                        error!("failed to send new slot execution output : {}", e);
                                        break;
                                    }
                                }
                            },

                            Err(e) => error!("error on receive new slot execution output : {}", e)
                        }
                    },
                    // Receive a new message from the in_stream
                    res = in_stream.next() => {
                        match res {
                            Some(res) => {
                                match res {
                                    Ok(message) => {
                                        // Update current filter
                                        filters = match get_filter(message.clone(), &grpc_config) {
                                            Ok(filter) => filter,
                                            Err(err) => {
                                                error!("failed to get filter: {}", err);
                                                // Send the error response back to the client
                                                if let Err(e) = tx.send(Err(err.into())).await {
                                                    error!("failed to send back NewBlocks error response: {}", e);
                                                }
                                                return;
                                            }
                                        };
                                    },
                                    // Handle any errors that may occur during receiving the data
                                    Err(err) => {
                                        // Check if the error matches any IO errors
                                        if let Some(io_err) = match_for_io_error(&err) {
                                            if io_err.kind() == ErrorKind::BrokenPipe {
                                                warn!("client disconnected, broken pipe: {}", io_err);
                                                break;
                                            }
                                        }
                                        error!("{}", err);
                                        // Send the error response back to the client
                                        if let Err(e) = tx.send(Err(err)).await {
                                            error!("failed to send back new_slot_execution_outputs error response: {}", e);
                                            break;
                                        }
                                    }
                                }
                            },
                            None => {
                                // The client has disconnected
                                break;
                            },
                        }
                    }
                }
            }
        } else {
            error!("empty request");
        }
    });

    // Create a new stream from the received channel
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    // Return the new stream of slot execution output
    Ok(Box::pin(out_stream) as NewSlotExecutionOutputsStreamType)
}

// This function returns a filter from the request
fn get_filter(
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
                grpc_api::new_slot_execution_outputs_filter::Filter::SlotRange(s_range) => result.slot_ranges_filter = Some(s_range),
                grpc_api::new_slot_execution_outputs_filter::Filter::AsyncPoolChangesFilter(filter) => result.async_pool_changes_filter = filter.filter.into(),
                grpc_api::new_slot_execution_outputs_filter::Filter::ExecutedDenounciationFilter(filter) => result.executed_denounciation_filter = filter.filter.into(),
                grpc_api::new_slot_execution_outputs_filter::Filter::EventFilter(filter) => {
                    if let Some(request_f) = filter.filter {
                        result.execution_event_filter.get_or_insert(Vec::new()).push(request_f.into());
                    } else {
                        result.execution_event_filter = None;
                    }
                },
                grpc_api::new_slot_execution_outputs_filter::Filter::ExecutedOpsChangesFilter(filter) => result.executed_ops_changes_filter = filter.filter.into(),
                grpc_api::new_slot_execution_outputs_filter::Filter::LedgerChangesFilter(filter) => result.ledger_changes_filter = filter.filter.into(),
            }
        }
    }

    Ok(result)
}

/// Return if the slot execution outputs should be send to client
fn filter_map(
    slot_execution_output: SlotExecutionOutput,
    filters: &Filter,
    grpc_config: &GrpcConfig,
) -> Option<SlotExecutionOutput> {
    match &slot_execution_output {
        SlotExecutionOutput::ExecutedSlot(e_output) => {
            if let Some(status_filter) = &filters.status_filter {
                let id = grpc_model::ExecutionOutputStatus::Candidate as i32;

                if !status_filter.eq(&id) {
                    return None;
                }
            }

            filter_map_exec_output(e_output.clone(), filters, grpc_config)
                .map(SlotExecutionOutput::ExecutedSlot)
        }
        SlotExecutionOutput::FinalizedSlot(e_output) => {
            if let Some(status_filter) = &filters.status_filter {
                let id = grpc_model::ExecutionOutputStatus::Final as i32;

                if !status_filter.eq(&id) {
                    return None;
                }
            }

            filter_map_exec_output(e_output.clone(), filters, grpc_config)
                .map(SlotExecutionOutput::FinalizedSlot)
        }
    }
}

// Return if the execution outputs should be send and remove the fields that are not needed
fn filter_map_exec_output(
    mut exec_output: ExecutionOutput,
    filters: &Filter,
    grpc_config: &GrpcConfig,
) -> Option<ExecutionOutput> {
    if let Some(slot_ranges) = &filters.slot_ranges_filter {
        if let Some(start) = slot_ranges.start_slot {
            if exec_output.slot < start.into() {
                return None;
            }
        }

        if let Some(end) = slot_ranges.end_slot {
            if exec_output.slot >= end.into() {
                return None;
            }
        }
    }

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

    if let Some(async_pool_changes_filter) = &filters.async_pool_changes_filter {
        exec_output.state_changes.async_pool_changes.0.retain(
            |(_msg_id, _slot, _emission_index), changes| match async_pool_changes_filter {
                grpc_api::async_pool_changes_filter::Filter::None(_empty) => return false,
                grpc_api::async_pool_changes_filter::Filter::Type(filter_type) => match changes {
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
                grpc_api::async_pool_changes_filter::Filter::Handler(handler) => match changes {
                    massa_models::types::SetUpdateOrDelete::Set(msg) => msg.function.eq(handler),
                    massa_models::types::SetUpdateOrDelete::Update(msg) => match &msg.function {
                        massa_models::types::SetOrKeep::Set(func) => func.eq(handler),
                        massa_models::types::SetOrKeep::Keep => false,
                    },
                    massa_models::types::SetUpdateOrDelete::Delete => false,
                },
                grpc_api::async_pool_changes_filter::Filter::DestinationAddress(
                    filter_dest_addr,
                ) => match changes {
                    massa_models::types::SetUpdateOrDelete::Set(msg) => {
                        msg.destination.to_string().eq(filter_dest_addr)
                    }
                    massa_models::types::SetUpdateOrDelete::Update(msg) => match msg.destination {
                        massa_models::types::SetOrKeep::Set(dest) => {
                            dest.to_string().eq(filter_dest_addr)
                        }
                        massa_models::types::SetOrKeep::Keep => false,
                    },
                    massa_models::types::SetUpdateOrDelete::Delete => false,
                },
                grpc_api::async_pool_changes_filter::Filter::EmitterAddress(filter_emit_addr) => {
                    match changes {
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
                    }
                }
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
            },
        );

        if exec_output.state_changes.async_pool_changes.0.is_empty() {
            return None;
        }
    }

    if let Some(executed_denounciation_filter) = &filters.executed_denounciation_filter {
        match executed_denounciation_filter {
            grpc_api::executed_denounciation_filter::Filter::None(_empty) => return None,
        }
    }

    if let Some(executed_ops_changes_filter) = &filters.executed_ops_changes_filter {
        exec_output
            .state_changes
            .executed_ops_changes
            .retain(|op, (_success, _slot)| match executed_ops_changes_filter {
                grpc_api::executed_ops_changes_filter::Filter::None(_) => false,
                grpc_api::executed_ops_changes_filter::Filter::OperationId(filter_op_id) => {
                    return op.to_string().eq(filter_op_id);
                }
            });

        if exec_output.state_changes.executed_ops_changes.is_empty() {
            return None;
        }
    }

    if let Some(ledger_changes_filter) = &filters.ledger_changes_filter {
        exec_output
            .state_changes
            .ledger_changes
            .0
            .retain(|addr, _ledger_change| match ledger_changes_filter {
                grpc_api::ledger_changes_filter::Filter::None(_) => false,
                grpc_api::ledger_changes_filter::Filter::Address(filter_addr) => {
                    return addr.to_string().eq(filter_addr);
                }
            });

        if exec_output.state_changes.ledger_changes.0.is_empty() {
            return None;
        }
    }

    Some(exec_output)
}
