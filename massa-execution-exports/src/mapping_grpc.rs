// Copyright (c) 2023 MASSA LABS <info@massa.net>

use std::str::FromStr;

use crate::execution_info::ExecutionInfoForSlot;

#[cfg(feature = "execution-info")]
use crate::execution_info::{TransferContext, TransferType};
use crate::{
    ExecutionOutput, ExecutionQueryCycleInfos, ExecutionQueryError, ExecutionQueryExecutionStatus,
    ExecutionQueryRequestItem, ExecutionQueryResponseItem, ExecutionQueryStakerInfo,
    SlotExecutionOutput,
};
use grpc_api::execution_query_request_item as exec;
use massa_models::address::Address;
use massa_models::datastore::cleanup_datastore_key_range_query;
use massa_models::deferred_calls::DeferredCallId;
use massa_models::error::ModelsError;
use massa_models::execution::EventFilter;
use massa_models::mapping_grpc::to_denunciation_index;
use massa_models::operation::OperationId;
use massa_models::prehash::{CapacityAllocator, PreHashSet};
use massa_proto_rs::massa::api::v1 as grpc_api;
#[cfg(feature = "execution-info")]
use massa_proto_rs::massa::model::v1::CoinOrigin;
use massa_proto_rs::massa::model::v1::{self as grpc_model};

/// Convert a `grpc_api::ScExecutionEventsRequest` to a `ScExecutionEventsRequest`
pub fn to_querystate_filter(
    query: grpc_api::ExecutionQueryRequestItem,
    network_version: u32,
    max_datastore_query_config: Option<u32>,
    max_datastore_key_length: u8,
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
            exec::RequestItem::AddressBytecodeFinal(value) => Ok(
                ExecutionQueryRequestItem::AddressBytecodeFinal(Address::from_str(&value.address)?),
            ),
            exec::RequestItem::AddressDatastoreKeysCandidate(value) => {
                let address = Address::from_str(&value.address)?;

                let start_key = match (value.start_key, value.inclusive_start_key.unwrap_or(true)) {
                    (None, _) => std::ops::Bound::Unbounded,
                    (Some(k), true) => std::ops::Bound::Included(k),
                    (Some(k), false) => std::ops::Bound::Excluded(k),
                };
                let end_key = match (value.end_key, value.inclusive_end_key.unwrap_or(true)) {
                    (None, _) => std::ops::Bound::Unbounded,
                    (Some(k), true) => std::ops::Bound::Included(k),
                    (Some(k), false) => std::ops::Bound::Excluded(k),
                };

                let (prefix, start_key, end_key) = cleanup_datastore_key_range_query(
                    &value.prefix,
                    start_key,
                    end_key,
                    value.limit,
                    max_datastore_key_length,
                    max_datastore_query_config,
                )?;

                Ok(ExecutionQueryRequestItem::AddressDatastoreKeysCandidate {
                    address,
                    prefix,
                    start_key,
                    end_key,
                    count: value.limit,
                })
            }
            exec::RequestItem::AddressDatastoreKeysFinal(value) => {
                let address = Address::from_str(&value.address)?;

                let start_key = match (value.start_key, value.inclusive_start_key.unwrap_or(true)) {
                    (None, _) => std::ops::Bound::Unbounded,
                    (Some(k), true) => std::ops::Bound::Included(k),
                    (Some(k), false) => std::ops::Bound::Excluded(k),
                };
                let end_key = match (value.end_key, value.inclusive_end_key.unwrap_or(true)) {
                    (None, _) => std::ops::Bound::Unbounded,
                    (Some(k), true) => std::ops::Bound::Included(k),
                    (Some(k), false) => std::ops::Bound::Excluded(k),
                };

                let (prefix, start_key, end_key) = cleanup_datastore_key_range_query(
                    &value.prefix,
                    start_key,
                    end_key,
                    value.limit,
                    max_datastore_key_length,
                    max_datastore_query_config,
                )?;

                Ok(ExecutionQueryRequestItem::AddressDatastoreKeysFinal {
                    address,
                    prefix,
                    start_key,
                    end_key,
                    count: value.limit,
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
                let addresses = value
                    .restrict_to_addresses
                    .into_iter()
                    .map(|address| Address::from_str(&address))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut addresses_set = PreHashSet::with_capacity(addresses.len());
                addresses_set.extend(addresses);
                Ok(ExecutionQueryRequestItem::CycleInfos {
                    cycle: value.cycle,
                    restrict_to_addresses: Some(addresses_set),
                })
            }
            exec::RequestItem::Events(value) => {
                let event_filter = to_event_filter(value.filters)?;
                Ok(ExecutionQueryRequestItem::Events(event_filter))
            }
            exec::RequestItem::DeferredCallQuote(value) => {
                if network_version < 1 {
                    return Err(ModelsError::InvalidVersionError(
                        "deferred call quote is not supported in this network version".to_string(),
                    ));
                }

                Ok(ExecutionQueryRequestItem::DeferredCallQuote {
                    target_slot: value
                        .target_slot
                        .ok_or(ModelsError::ErrorRaised(
                            "target slot is required".to_string(),
                        ))?
                        .into(),
                    max_gas_request: value.max_gas,
                    params_size: value.params_size,
                })
            }
            exec::RequestItem::DeferredCallInfo(info) => {
                if network_version < 1 {
                    return Err(ModelsError::InvalidVersionError(
                        "deferred call quote is not supported in this network version".to_string(),
                    ));
                }

                let id = DeferredCallId::from_str(&info.call_id)?;
                Ok(ExecutionQueryRequestItem::DeferredCallInfo(id))
            }
            exec::RequestItem::DeferredCallsBySlot(value) => {
                if network_version < 1 {
                    return Err(ModelsError::InvalidVersionError(
                        "deferred call quote is not supported in this network version".to_string(),
                    ));
                }

                Ok(ExecutionQueryRequestItem::DeferredCallsBySlot(
                    value
                        .slot
                        .ok_or(ModelsError::ErrorRaised("slot is required".to_string()))?
                        .into(),
                ))
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
        ExecutionQueryResponseItem::AddressDatastoreKeys(result, _address, _is_final) => {
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
        ExecutionQueryResponseItem::DeferredCallQuote(
            target_slot,
            max_gas_request,
            available,
            price,
        ) => grpc_api::execution_query_response_item::ResponseItem::DeferredCallQuote(
            grpc_api::DeferredCallQuoteResponse {
                target_slot: Some(target_slot.into()),
                max_gas_request,
                available,
                price: Some(price.into()),
            },
        ),
        ExecutionQueryResponseItem::DeferredCallInfo(call_id, call) => {
            grpc_api::execution_query_response_item::ResponseItem::DeferredCallInfo(
                grpc_api::DeferredCallInfoResponse {
                    call_id: call_id.to_string(),
                    call: Some(call.into()),
                },
            )
        }
        ExecutionQueryResponseItem::DeferredCallsBySlot(slot, ids) => {
            let arr = ids.into_iter().map(|id| id.to_string()).collect();
            grpc_api::execution_query_response_item::ResponseItem::DeferredCallsBySlot(
                grpc_api::DeferredCallsBySlotResponse {
                    slot: Some(slot.into()),
                    call_ids: arr,
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

impl From<ExecutionInfoForSlot> for grpc_api::NewTransfersInfoServerResponse {
    fn from(value: ExecutionInfoForSlot) -> Self {
        #[cfg(feature = "execution-info")]
        let transfers_info: Vec<grpc_model::TransferInfo> = value
            .transfers
            .into_iter()
            .map(|transfer| {
                let id = transfer.id.unwrap_or("Unknown id".to_string());

                let item = match transfer.t_type {
                    TransferType::Mas => grpc_model::Item::Mas as i32,
                    TransferType::Roll => grpc_model::Item::Roll as i32,
                    TransferType::DeferredCredits => grpc_model::Item::DeferredMas as i32,
                };

                let from_address = transfer.from.map(|a| a.to_string());
                let to_address = transfer.to.map(|a| a.to_string());
                let amount = transfer.amount.map(|a| a.to_raw());
                let rolls_count = transfer.roll_count.map(|a| a.into());
                let denunciation_index = transfer.denunciation_index.map(|a| a.into());

                let (origin, operation_id, async_msg_id, deferred_call_id) = match transfer.context
                {
                    TransferContext::TransactionCoins(ope_id) => (
                        CoinOrigin::OpTransactionCoins as i32,
                        Some(ope_id.to_string()),
                        None,
                        None,
                    ),
                    TransferContext::AyncMsgCancel(_msg_id, msg_id_str) => {
                        (CoinOrigin::AsyncMsgCancel as i32, None, msg_id_str, None)
                    }
                    TransferContext::DeferredCredits => {
                        (CoinOrigin::DeferredCredit as i32, None, None, None)
                    }
                    TransferContext::DeferredCallFail(call_id) => (
                        CoinOrigin::DeferredCallFail as i32,
                        None,
                        None,
                        Some(call_id.to_string()),
                    ),
                    TransferContext::DeferredCallCancel(call_id) => (
                        CoinOrigin::DeferredCallCancel as i32,
                        None,
                        None,
                        Some(call_id.to_string()),
                    ),
                    TransferContext::DeferredCallCoins(call_id) => (
                        CoinOrigin::DeferredCallCoins as i32,
                        None,
                        None,
                        Some(call_id.to_string()),
                    ),
                    TransferContext::DeferredCallRegister => {
                        (CoinOrigin::DeferredCallRegister as i32, None, None, None)
                    }
                    TransferContext::DeferredCallStorageRefund(call_id) => (
                        CoinOrigin::DeferredCallStorageRefund as i32,
                        None,
                        None,
                        Some(call_id.to_string()),
                    ),
                    TransferContext::OperationFee(ope_id) => (
                        CoinOrigin::OpTransactionFees as i32,
                        Some(ope_id.to_string()),
                        None,
                        None,
                    ),
                    TransferContext::RollBuy(ope_id) => (
                        CoinOrigin::OpRollBuy as i32,
                        Some(ope_id.to_string()),
                        None,
                        None,
                    ),
                    TransferContext::RollSell(ope_id) => (
                        CoinOrigin::OpRollSell as i32,
                        Some(ope_id.to_string()),
                        None,
                        None,
                    ),
                    TransferContext::RollSlash => (CoinOrigin::Slash as i32, None, None, None),
                    TransferContext::CreateSCStorage => {
                        (CoinOrigin::CreateScStorage as i32, None, None, None)
                    }
                    TransferContext::DatastoreStorage => {
                        (CoinOrigin::DatastoreStorage as i32, None, None, None)
                    }
                    TransferContext::CallSCCoins(ope_id) => (
                        CoinOrigin::OpCallscCoins as i32,
                        Some(ope_id.to_string()),
                        None,
                        None,
                    ),
                    TransferContext::AsyncMsgCoins(_msg_id, msg_id_str) => {
                        (CoinOrigin::AsyncMsgCoins as i32, None, msg_id_str, None)
                    }
                    TransferContext::EndorsementCreatorReward => {
                        (CoinOrigin::EndorsementReward as i32, None, None, None)
                    }
                    TransferContext::EndorsementTargetReward => {
                        (CoinOrigin::EndorsedReward as i32, None, None, None)
                    }
                    TransferContext::BlockCreatorReward => {
                        (CoinOrigin::BlockReward as i32, None, None, None)
                    }
                    TransferContext::ReadOnlyBytecodeExecutionFee => (
                        CoinOrigin::ReadOnlyBytecodeExecFees as i32,
                        None,
                        None,
                        None,
                    ),
                    TransferContext::ReadOnlyFunctionCallFee => {
                        (CoinOrigin::ReadOnlyFnCallFees as i32, None, None, None)
                    }
                    TransferContext::ReadOnlyFunctionCallCoins => {
                        (CoinOrigin::ReadOnlyFnCallCoins as i32, None, None, None)
                    }
                    TransferContext::SetBytecodeStorage => {
                        (CoinOrigin::SetBytecodeStorage as i32, None, None, None)
                    }
                    TransferContext::AbiCallCoins => {
                        (CoinOrigin::AbiCallCoins as i32, None, None, None)
                    }
                    TransferContext::AbiTransferCoins => {
                        (CoinOrigin::AbiTransferCoins as i32, None, None, None)
                    }
                    TransferContext::AbiTransferForCoins => {
                        (CoinOrigin::AbiTransferForCoins as i32, None, None, None)
                    }
                    TransferContext::AbiSendMsgCoins => {
                        (CoinOrigin::AbiSendMsgCoins as i32, None, None, None)
                    }
                    TransferContext::AbiSendMsgFee => {
                        (CoinOrigin::AbiSendMsgFees as i32, None, None, None)
                    }
                };

                grpc_model::TransferInfo {
                    id,
                    from_address,
                    to_address,
                    item,
                    amount,
                    rolls_count,
                    origin,
                    operation_id,
                    async_msg_id,
                    deferred_call_id,
                    denunciation_index,
                }
            })
            .collect();

        grpc_api::NewTransfersInfoServerResponse {
            slot: Some(value.slot.into()),
            timestamp: value.timestamp.as_millis() as i64,
            block_id: value.opt_block_id.map(|b| b.to_string()),
            #[cfg(feature = "execution-info")]
            transfers_info,
            #[cfg(not(feature = "execution-info"))]
            transfers_info: vec![],
        }
    }
}
