// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::config::GrpcConfig;
use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use crate::SlotRange;
use futures_util::StreamExt;
use massa_execution_exports::{ExecutionOutput, SlotExecutionOutput};
use massa_models::slot::Slot;
use massa_proto_rs::massa::api::v1::{self as grpc_api, NewSlotExecutionOutputsRequest};
use massa_proto_rs::massa::model::v1::{self as grpc_model};
use std::collections::HashSet;
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
    status_filter: Option<HashSet<i32>>,
    // Slot range to filter
    slot_ranges_filter: Option<HashSet<SlotRange>>,
    // Async pool changes filter
    async_pool_changes_filter: Option<AsyncPoolChangesFilter>,
    // Executed denounciation filter
    executed_denounciation_filter: Option<ExecutedDenounciationFilter>,
    // Execution event filter
    execution_event_filter: Option<ExecutionEventFilter>,
    // Executed ops changes filter
    executed_ops_changes_filter: Option<ExecutedOpsChangesFilter>,
    // Ledger changes filter
    ledger_changes_filter: Option<LedgerChangesFilter>,
}

#[derive(Clone, Debug, Default)]
struct AsyncPoolChangesFilter {
    // Do not return any message
    none: Option<()>,
}

#[derive(Clone, Debug, Default)]
struct ExecutedDenounciationFilter {
    // Do not return any message
    none: Option<()>,
}

#[derive(Clone, Debug, Default)]
struct ExecutionEventFilter {
    // Do not return any message
    none: Option<()>,
}

#[derive(Clone, Debug, Default)]
struct ExecutedOpsChangesFilter {
    // Do not return any message
    none: Option<()>,
}

#[derive(Clone, Debug, Default)]
struct LedgerChangesFilter {
    // Do not return any message
    none: Option<()>,
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

    let mut status_filter: Option<HashSet<i32>> = None;
    let mut slot_ranges_filter: Option<HashSet<SlotRange>> = None;
    let mut async_pool_changes_filter: Option<AsyncPoolChangesFilter> = None;
    let mut executed_denounciation_filter: Option<ExecutedDenounciationFilter> = None;
    let mut execution_event_filter: Option<ExecutionEventFilter> = None;
    let mut executed_ops_changes_filter: Option<ExecutedOpsChangesFilter> = None;
    let mut ledger_changes_filter: Option<LedgerChangesFilter> = None;

    for query in request.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::new_slot_execution_outputs_filter::Filter::Status(status) => {
                    let statuses = status_filter.get_or_insert_with(HashSet::new);
                    // The limit is the number of valid enum values
                    if statuses.len() as u32 > 3 {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many statuses received. Only a maximum of {} statuses are accepted per request",
                            2
                        )));
                    }
                    statuses.insert(status);
                },
                grpc_api::new_slot_execution_outputs_filter::Filter::SlotRange(s_range) => {
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
                },
                grpc_api::new_slot_execution_outputs_filter::Filter::AsyncPoolChangesFilter(filter) => {
                    if let Some(filter) = filter.filter {
                        match filter {
                            grpc_api::async_pool_changes_filter::Filter::None(_) => {
                                async_pool_changes_filter = Some(AsyncPoolChangesFilter {
                                    none: Some(()),
                                });
                            },
                            _ => {
                                async_pool_changes_filter = Some(AsyncPoolChangesFilter {
                                none: None,
                            })
                        }
                    }
                }
            },
                grpc_api::new_slot_execution_outputs_filter::Filter::ExecutedDenounciationFilter(filter) => {
                    if let Some(filter) = filter.filter {
                        match filter {
                            grpc_api::executed_denounciation_filter::Filter::None(_) => {
                                executed_denounciation_filter = Some(ExecutedDenounciationFilter {
                                    none: Some(()),
                                });
                            },
                    }
                }},
                grpc_api::new_slot_execution_outputs_filter::Filter::EventFilter(filter) => {
                    if let Some(filter) = filter.filter {
                        match filter {
                            grpc_api::execution_event_filter::Filter::None(_) => {
                                execution_event_filter = Some(ExecutionEventFilter {
                                    none: Some(()),
                                });
                            },
                            _ => {
                                execution_event_filter = Some(ExecutionEventFilter {
                                none: None,
                            })
                        }
                }
                }},
                grpc_api::new_slot_execution_outputs_filter::Filter::ExecutedOpsChangesFilter(filter) => {
                    if let Some(filter) = filter.filter {
                        match filter {
                            grpc_api::executed_ops_changes_filter::Filter::None(_) => {
                                executed_ops_changes_filter = Some(ExecutedOpsChangesFilter {
                                    none: Some(()),
                                });
                            },
                            _ => {
                                executed_ops_changes_filter = Some(ExecutedOpsChangesFilter {
                                none: None,
                            })
                        }
                }
                }},
                grpc_api::new_slot_execution_outputs_filter::Filter::LedgerChangesFilter(filter) => {
                    if let Some(filter) = filter.filter {
                        match filter {
                            grpc_api::ledger_changes_filter::Filter::None(_) => {
                                ledger_changes_filter = Some(LedgerChangesFilter {
                                    none: Some(()),
                                });
                            },
                            _ => {
                                ledger_changes_filter = Some(LedgerChangesFilter {
                                none: None,
                            })
                        }
                }
                }},
            }
        }
    }

    Ok(Filter {
        status_filter,
        slot_ranges_filter,
        async_pool_changes_filter,
        executed_denounciation_filter,
        execution_event_filter,
        executed_ops_changes_filter,
        ledger_changes_filter,
    })
}

/// Return if the slot execution outputs should be send to client
fn filter_map(
    slot_execution_output: SlotExecutionOutput,
    filters: &Filter,
    grpc_config: &GrpcConfig,
) -> Option<SlotExecutionOutput> {
    match &slot_execution_output {
        SlotExecutionOutput::ExecutedSlot(e_output) => {
            let id = grpc_model::ExecutionOutputStatus::Candidate as i32;
            if let Some(status_filter) = &filters.status_filter {
                if !status_filter.contains(&id) {
                    return None;
                }
            }
            filter_map_exec_output(e_output.clone(), filters, grpc_config)
                .map(SlotExecutionOutput::ExecutedSlot)
        }
        SlotExecutionOutput::FinalizedSlot(e_output) => {
            let id = grpc_model::ExecutionOutputStatus::Final as i32;
            if let Some(status_filter) = &filters.status_filter {
                if !status_filter.contains(&id) {
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
        let current_slot = exec_output.slot;

        if current_slot < start_slot || current_slot >= end_slot {
            return None;
        }
    }

    if let Some(slot_ranges) = &filters.slot_ranges_filter {
        let slot_changes_matches = slot_ranges.iter().any(|slot_range| {
            let start_slot_check = slot_range
                .start_slot
                .map_or(true, |start_slot| exec_output.slot >= start_slot);
            let end_slot_check = slot_range
                .end_slot
                .map_or(true, |end_slot| exec_output.slot <= end_slot);

            start_slot_check && end_slot_check
        });

        if !slot_changes_matches {
            return None;
        }
    }

    if let Some(execution_event_filter) = &filters.execution_event_filter {
        if execution_event_filter.none.is_some() {
            exec_output.events.clear();
        }
    }

    if let Some(async_pool_changes_filter) = &filters.async_pool_changes_filter {
        if async_pool_changes_filter.none.is_some() {
            exec_output.state_changes.async_pool_changes.0.clear();
        }
    }
    if let Some(executed_denounciation_filter) = &filters.executed_denounciation_filter {
        if executed_denounciation_filter.none.is_some() {
            exec_output
                .state_changes
                .executed_denunciations_changes
                .clear();
        }
    }
    if let Some(executed_ops_changes_filter) = &filters.executed_ops_changes_filter {
        if executed_ops_changes_filter.none.is_some() {
            exec_output.state_changes.executed_ops_changes.clear();
        }
    }
    if let Some(ledger_changes_filter) = &filters.ledger_changes_filter {
        if ledger_changes_filter.none.is_some() {
            exec_output.state_changes.ledger_changes.0.clear();
        }
    }

    Some(exec_output)
}
