// Copyright (c) 2023 MASSA LABS <info@massa.net>

use std::str::FromStr;

use crate::{
    ExecutionOutput, ExecutionQueryCycleInfos, ExecutionQueryError, ExecutionQueryExecutionStatus,
    ExecutionQueryRequestItem, ExecutionQueryResponseItem, ExecutionQueryStakerInfo,
    SlotExecutionOutput,
};
use grpc_api::execution_query_request_item as exec;
use massa_models::address::Address;
use massa_models::error::ModelsError;
use massa_models::execution::EventFilter;
use massa_models::mapping_grpc::to_denunciation_index;
use massa_models::operation::OperationId;
use massa_models::prehash::{CapacityAllocator, PreHashSet};
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1 as grpc_model;

/// Convert a `grpc_api::ScExecutionEventsRequest` to a `ScExecutionEventsRequest`
pub fn to_querystate_filter(
    query: grpc_api::ExecutionQueryRequestItem,
) -> Result<ExecutionQueryRequestItem, ModelsError> {
    if let Some(item) = query.request_item {
        match item {
            exec::RequestItem::AddressExistsCandidate(value) => {
                Ok(ExecutionQueryRequestItem::AddressExistsCandidate(
                    Address::from_str(&value.address)?,
                ))
            }
            exec::RequestItem::AddressExistsFinal(value) => Ok(
                ExecutionQueryRequestItem::AddressExistsFinal(Address::from_str(&value.address)?),
            ),
            exec::RequestItem::AddressBalanceCandidate(value) => {
                Ok(ExecutionQueryRequestItem::AddressBalanceCandidate(
                    Address::from_str(&value.address)?,
                ))
            }
            exec::RequestItem::AddressBalanceFinal(value) => Ok(
                ExecutionQueryRequestItem::AddressBalanceFinal(Address::from_str(&value.address)?),
            ),
            exec::RequestItem::AddressBytecodeCandidate(value) => {
                Ok(ExecutionQueryRequestItem::AddressBytecodeCandidate(
                    Address::from_str(&value.address)?,
                ))
            }
            exec::RequestItem::AddressBytecodeFinal(value) => {
                Ok(ExecutionQueryRequestItem::AddressBytecodeCandidate(
                    Address::from_str(&value.address)?,
                ))
            }
            exec::RequestItem::AddressDatastoreKeysCandidate(value) => {
                Ok(ExecutionQueryRequestItem::AddressBytecodeCandidate(
                    Address::from_str(&value.address)?,
                ))
            }
            exec::RequestItem::AddressDatastoreKeysFinal(value) => {
                Ok(ExecutionQueryRequestItem::AddressDatastoreKeysFinal {
                    addr: Address::from_str(&value.address)?,
                    prefix: value.prefix,
                })
            }
            exec::RequestItem::AddressDatastoreValueCandidate(value) => {
                Ok(ExecutionQueryRequestItem::AddressDatastoreValueCandidate {
                    addr: Address::from_str(&value.address)?,
                    key: value.key,
                })
            }
            exec::RequestItem::AddressDatastoreValueFinal(value) => {
                Ok(ExecutionQueryRequestItem::AddressDatastoreValueFinal {
                    addr: Address::from_str(&value.address)?,
                    key: value.key,
                })
            }
            exec::RequestItem::OpExecutionStatusCandidate(value) => {
                Ok(ExecutionQueryRequestItem::OpExecutionStatusCandidate(
                    OperationId::from_str(&value.operation_id)?,
                ))
            }
            exec::RequestItem::OpExecutionStatusFinal(value) => {
                Ok(ExecutionQueryRequestItem::OpExecutionStatusFinal(
                    OperationId::from_str(&value.operation_id)?,
                ))
            }
            //TODO to be improved
            exec::RequestItem::DenunciationExecutionStatusCandidate(value) => Ok(
                ExecutionQueryRequestItem::DenunciationExecutionStatusCandidate(
                    to_denunciation_index(value.denunciation_index.ok_or_else(|| {
                        ModelsError::ErrorRaised("no denounciation index found".to_string())
                    })?)?,
                ),
            ),
            //TODO to be improved
            exec::RequestItem::DenunciationExecutionStatusFinal(value) => {
                Ok(ExecutionQueryRequestItem::DenunciationExecutionStatusFinal(
                    to_denunciation_index(value.denunciation_index.ok_or_else(|| {
                        ModelsError::ErrorRaised("no denounciation index found".to_string())
                    })?)?,
                ))
            }
            exec::RequestItem::AddressRollsCandidate(value) => {
                Ok(ExecutionQueryRequestItem::AddressRollsCandidate(
                    Address::from_str(&value.address)?,
                ))
            }
            exec::RequestItem::AddressRollsFinal(value) => Ok(
                ExecutionQueryRequestItem::AddressRollsFinal(Address::from_str(&value.address)?),
            ),
            exec::RequestItem::AddressDeferredCreditsCandidate(value) => {
                Ok(ExecutionQueryRequestItem::AddressDeferredCreditsCandidate(
                    Address::from_str(&value.address)?,
                ))
            }
            exec::RequestItem::AddressDeferredCreditsFinal(value) => {
                Ok(ExecutionQueryRequestItem::AddressDeferredCreditsFinal(
                    Address::from_str(&value.address)?,
                ))
            }
            //TODO to be checked
            exec::RequestItem::CycleInfos(value) => {
                let addreses = value
                    .restrict_to_addresses
                    .into_iter()
                    .map(|address| Address::from_str(&address))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut addresses_set = PreHashSet::with_capacity(addreses.len());
                addresses_set.extend(addreses);
                Ok(ExecutionQueryRequestItem::CycleInfos {
                    cycle: value.cycle,
                    restrict_to_addresses: Some(addresses_set),
                })
            }
            exec::RequestItem::Events(value) => {
                let event_filter = to_event_filter(value.filters)?;
                Ok(ExecutionQueryRequestItem::Events(event_filter))
            }
        }
    } else {
        Err(ModelsError::ErrorRaised("no filter provided".to_string()))
    }
}

/// Convert a vector of `grpc_model::ScExecutionEventsFilter` to a `EventFilter`
pub fn to_event_filter(
    sce_filters: Vec<grpc_api::ScExecutionEventsFilter>,
) -> Result<EventFilter, ModelsError> {
    let mut event_filter = EventFilter::default();
    for query in sce_filters {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::sc_execution_events_filter::Filter::SlotRange(slot_range) => {
                    event_filter.start = slot_range.start_slot.map(|slot| slot.into());
                    event_filter.end = slot_range.end_slot.map(|slot| slot.into());
                }
                grpc_api::sc_execution_events_filter::Filter::CallerAddress(caller_address) => {
                    event_filter.original_caller_address =
                        Some(Address::from_str(&caller_address)?);
                }
                grpc_api::sc_execution_events_filter::Filter::EmitterAddress(emitter_address) => {
                    event_filter.emitter_address = Some(Address::from_str(&emitter_address)?);
                }
                grpc_api::sc_execution_events_filter::Filter::OriginalOperationId(operation_id) => {
                    event_filter.original_operation_id =
                        Some(OperationId::from_str(&operation_id)?);
                }
                grpc_api::sc_execution_events_filter::Filter::IsFailure(is_failure) => {
                    event_filter.is_error = Some(is_failure);
                }
                grpc_api::sc_execution_events_filter::Filter::Status(status) => {
                    // See grpc_model::ScExecutionEventStatus
                    match status {
                        1 => event_filter.is_final = Some(true),
                        2 => event_filter.is_final = Some(false),
                        _ => event_filter.is_final = None,
                    }
                }
            }
        }
    }

    Ok(event_filter)
}

/// Converts a `ExecutionQueryResponse` to a `grpc_api::ExecutionQueryResponse`
pub fn to_execution_query_response(
    value: Result<ExecutionQueryResponseItem, ExecutionQueryError>,
) -> grpc_api::ExecutionQueryResponse {
    match value {
        Ok(item) => grpc_api::ExecutionQueryResponse {
            response: Some(grpc_api::execution_query_response::Response::Result(
                to_execution_query_result(item),
            )),
        },
        Err(err) => grpc_api::ExecutionQueryResponse {
            response: Some(grpc_api::execution_query_response::Response::Error(
                err.into(),
            )),
        },
    }
}

// Convertss a `ExecutionQueryResponseItem` to a `grpc_api::ExecutionQueryResponseItem`
fn to_execution_query_result(
    value: ExecutionQueryResponseItem,
) -> grpc_api::ExecutionQueryResponseItem {
    let response_item = match value {
        ExecutionQueryResponseItem::Boolean(result) => {
            grpc_api::execution_query_response_item::ResponseItem::Boolean(result)
        }
        ExecutionQueryResponseItem::RollCount(result) => {
            grpc_api::execution_query_response_item::ResponseItem::RollCount(result)
        }
        ExecutionQueryResponseItem::Amount(result) => {
            grpc_api::execution_query_response_item::ResponseItem::Amount(result.into())
        }
        ExecutionQueryResponseItem::Bytecode(result) => {
            grpc_api::execution_query_response_item::ResponseItem::Bytes(result.0)
        }
        ExecutionQueryResponseItem::DatastoreValue(result) => {
            grpc_api::execution_query_response_item::ResponseItem::Bytes(result)
        }
        ExecutionQueryResponseItem::KeyList(result) => {
            grpc_api::execution_query_response_item::ResponseItem::VecBytes(
                grpc_model::ArrayOfBytesWrapper {
                    items: result.into_iter().collect(),
                },
            )
        }
        ExecutionQueryResponseItem::DeferredCredits(result) => {
            grpc_api::execution_query_response_item::ResponseItem::DeferredCredits(
                grpc_api::DeferredCreditsEntryWrapper {
                    entries: result
                        .into_iter()
                        .map(|(slot, amount)| grpc_api::DeferredCreditsEntry {
                            slot: Some(slot.into()),
                            amount: Some(amount.into()),
                        })
                        .collect(),
                },
            )
        }
        ExecutionQueryResponseItem::ExecutionStatus(result) => match result {
            ExecutionQueryExecutionStatus::AlreadyExecutedWithSuccess => {
                grpc_api::execution_query_response_item::ResponseItem::ExecutionStatus(
                    grpc_api::ExecutionQueryExecutionStatus::AlreadyExecutedWithSuccess as i32,
                )
            }
            ExecutionQueryExecutionStatus::AlreadyExecutedWithFailure => {
                grpc_api::execution_query_response_item::ResponseItem::ExecutionStatus(
                    grpc_api::ExecutionQueryExecutionStatus::AlreadyExecutedWithFailure as i32,
                )
            }
            ExecutionQueryExecutionStatus::ExecutableOrExpired => {
                grpc_api::execution_query_response_item::ResponseItem::ExecutionStatus(
                    grpc_api::ExecutionQueryExecutionStatus::ExecutableOrExpired as i32,
                )
            }
        },
        ExecutionQueryResponseItem::CycleInfos(result) => {
            grpc_api::execution_query_response_item::ResponseItem::CycleInfos(to_cycle_info(result))
        }
        ExecutionQueryResponseItem::Events(result) => {
            grpc_api::execution_query_response_item::ResponseItem::Events(
                grpc_api::ScOutputEventsWrapper {
                    events: result.into_iter().map(|event| event.into()).collect(),
                },
            )
        }
    };

    grpc_api::ExecutionQueryResponseItem {
        response_item: Some(response_item),
    }
}

// Convertss a `ExecutionQueryCycleInfos` to a `grpc_api::CycleInfos`
fn to_cycle_info(value: ExecutionQueryCycleInfos) -> grpc_api::ExecutionQueryCycleInfos {
    grpc_api::ExecutionQueryCycleInfos {
        cycle: value.cycle,
        is_final: value.is_final,
        staker_infos: value
            .staker_infos
            .into_iter()
            .map(|(address, info)| to_execution_query_staker_info(address, info))
            .collect(),
    }
}

// Convertss a `ExecutionQueryStakerInfo` to a `grpc_api::ExecutionQueryStakerInfo`
fn to_execution_query_staker_info(
    address: Address,
    info: ExecutionQueryStakerInfo,
) -> grpc_api::ExecutionQueryStakerInfoEntry {
    grpc_api::ExecutionQueryStakerInfoEntry {
        address: address.to_string(),
        info: Some(grpc_api::ExecutionQueryStakerInfo {
            active_rolls: info.active_rolls,
            production_stats: Some(grpc_api::ExecutionQueryStakerInfoProductionStatsEntry {
                address: address.to_string(),
                stats: Some(grpc_api::ExecutionQueryStakerInfoProductionStats {
                    block_success_count: info.production_stats.block_success_count,
                    block_failure_count: info.production_stats.block_failure_count,
                }),
            }),
        }),
    }
}

impl From<SlotExecutionOutput> for grpc_model::SlotExecutionOutput {
    fn from(value: SlotExecutionOutput) -> Self {
        match value {
            SlotExecutionOutput::ExecutedSlot(execution_output) => {
                grpc_model::SlotExecutionOutput {
                    status: grpc_model::ExecutionOutputStatus::Candidate as i32,
                    execution_output: Some(execution_output.into()),
                }
            }
            SlotExecutionOutput::FinalizedSlot(execution_output) => {
                grpc_model::SlotExecutionOutput {
                    status: grpc_model::ExecutionOutputStatus::Final as i32,
                    execution_output: Some(execution_output.into()),
                }
            }
        }
    }
}

impl From<ExecutionOutput> for grpc_model::ExecutionOutput {
    fn from(value: ExecutionOutput) -> Self {
        grpc_model::ExecutionOutput {
            slot: Some(value.slot.into()),
            block_id: value.block_info.map(|i| i.block_id.to_string()),
            events: value
                .events
                .0
                .into_iter()
                .map(|event| event.into())
                .collect(),
            state_changes: Some(value.state_changes.into()),
        }
    }
}

impl From<ExecutionQueryError> for grpc_model::Error {
    fn from(value: ExecutionQueryError) -> Self {
        match value {
            ExecutionQueryError::NotFound(error) => grpc_model::Error {
                //TODO to be defined
                code: 404,
                message: error,
            },
        }
    }
}
