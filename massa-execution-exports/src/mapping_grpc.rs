// Copyright (c) 2023 MASSA LABS <info@massa.net>

use std::str::FromStr;

use crate::execution_info::ExecutionInfoForSlot;

use crate::{
    ExecutionOutput, ExecutionQueryCycleInfos, ExecutionQueryError, ExecutionQueryExecutionStatus,
    ExecutionQueryRequestItem, ExecutionQueryResponseItem, ExecutionQueryStakerInfo,
    SlotExecutionOutput,
};
use grpc_api::execution_query_request_item as exec;
use massa_models::address::Address;
use massa_models::amount::{self, Amount};
use massa_models::datastore::cleanup_datastore_key_range_query;
use massa_models::deferred_calls::DeferredCallId;
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

impl From<ExecutionInfoForSlot> for grpc_api::NewExecutionInfoServerResponse {
    fn from(value: ExecutionInfoForSlot) -> Self {
        let mut result: Vec<grpc_model::ExecutionInfo> = Vec::new();

        #[cfg(feature = "execution-trace")]
        {
            if let Some(vec_transfer) = value.transfers {
                result.extend(vec_transfer.into_iter().flat_map(|transfer| {
                    let common_fields =
                        |id: String,
                         address: String,
                         amount: Amount,
                         orig: grpc_model::CoinOrigin,
                         direction: grpc_model::CoinDirection| {
                            grpc_model::ExecutionInfo {
                                id,
                                address,
                                direction: direction as i32,
                                item: grpc_model::Item::Mas as i32,
                                prev_amount: todo!(),
                                amount: Some(amount.into()),
                                post_amount: todo!(),
                                timestamp: todo!(),
                                slot: Some(value.slot.into()),
                                origin: orig as i32,
                            }
                        };

                    vec![
                        common_fields(
                            todo!(),
                            transfer.from.to_string(),
                            transfer.amount,
                            grpc_model::CoinOrigin::OpTransactionCoins,
                            grpc_model::CoinDirection::Debit,
                        ),
                        common_fields(
                            todo!(),
                            transfer.from.to_string(),
                            transfer.fee,
                            grpc_model::CoinOrigin::OpTransactionFees,
                            grpc_model::CoinDirection::Debit,
                        ),
                        common_fields(
                            todo!(),
                            transfer.to.to_string(),
                            transfer.amount,
                            grpc_model::CoinOrigin::OpTransactionCoins,
                            grpc_model::CoinDirection::Credit,
                        ),
                    ]
                }));
            }
        }

        if let Some((addr, amount)) = value.block_producer_reward {
            result.push(grpc_model::ExecutionInfo {
                id: todo!(),
                address: addr.to_string(),
                direction: grpc_model::CoinDirection::Credit as i32,
                item: grpc_model::Item::Mas as i32,
                prev_amount: todo!(),
                amount: Some(amount.into()),
                post_amount: todo!(),
                timestamp: todo!(),
                slot: Some(value.slot.into()),
                origin: grpc_model::CoinOrigin::BlockReward as i32,
            });
        }

        result.extend(
            value
                .endorsement_creator_rewards
                .into_iter()
                .map(|(addr, amount)| grpc_model::ExecutionInfo {
                    id: todo!(),
                    address: addr.to_string(),
                    direction: grpc_model::CoinDirection::Credit as i32,
                    item: grpc_model::Item::Mas as i32,
                    prev_amount: todo!(),
                    amount: Some(amount.into()),
                    post_amount: todo!(),
                    timestamp: todo!(),
                    slot: Some(value.slot.into()),
                    origin: grpc_model::CoinOrigin::EndorsementReward as i32,
                }),
        );

        if let Some((addr, amount)) = value.endorsement_target_reward {
            result.push(grpc_model::ExecutionInfo {
                id: todo!(),
                address: addr.to_string(),
                direction: grpc_model::CoinDirection::Credit as i32,
                item: grpc_model::Item::Mas as i32,
                prev_amount: todo!(),
                amount: Some(amount.into()),
                post_amount: todo!(),
                timestamp: todo!(),
                slot: Some(value.slot.into()),
                origin: grpc_model::CoinOrigin::EndorsedReward as i32,
            });
        }

        let denunciations: Vec<grpc_model::ExecutionInfo> = value
            .denunciations
            .into_iter()
            .filter_map(|result| match result {
                Ok(den) => Some(grpc_model::ExecutionInfo {
                    id: todo!(),
                    address: den.address_denounced.to_string(),
                    direction: todo!(),
                    item: grpc_model::Item::Mas as i32,
                    prev_amount: todo!(),
                    amount: Some(den.slashed.into()),
                    post_amount: todo!(),
                    timestamp: todo!(),
                    slot: Some(value.slot.into()),
                    origin: todo!(),
                }),
                Err(_) => None,
            })
            .collect();

        result.extend(denunciations);

        let operations: Vec<grpc_model::ExecutionInfo> = value
            .operations
            .into_iter()
            .map(|op| match op {
                crate::execution_info::OperationInfo::RollBuy(addr, amount) => {
                    grpc_model::ExecutionInfo {
                        id: todo!(),
                        address: addr.to_string(),
                        direction: grpc_model::CoinDirection::Debit as i32,
                        item: grpc_model::Item::Mas as i32,
                        prev_amount: todo!(),
                        amount: Some(Amount::from_raw(amount).into()),
                        post_amount: todo!(),
                        timestamp: todo!(),
                        slot: Some(value.slot.into()),
                        origin: grpc_model::CoinOrigin::OpRollBuyRolls as i32,
                    }
                }
                crate::execution_info::OperationInfo::RollSell(addr, amount) => {
                    grpc_model::ExecutionInfo {
                        id: todo!(),
                        address: addr.to_string(),
                        direction: grpc_model::CoinDirection::Credit as i32,
                        item: grpc_model::Item::Mas as i32,
                        prev_amount: todo!(),
                        amount: Some(Amount::from_raw(amount).into()),
                        post_amount: todo!(),
                        timestamp: todo!(),
                        slot: Some(value.slot.into()),
                        origin: grpc_model::CoinOrigin::OpRollSellRolls as i32,
                    }
                }
            })
            .collect();

        result.extend(operations);

        result.extend(
            value
                .async_messages
                .into_iter()
                .filter_map(|msg| msg.ok())
                .flat_map(|msg| {
                    // TODO add fee debit for sender

                    let coins = msg.coins.map(|a| a.into());
                    let common_fields =
                        |id: String, address: String, direction: grpc_model::CoinDirection| {
                            grpc_model::ExecutionInfo {
                                id,
                                address,
                                direction: direction as i32,
                                item: grpc_model::Item::Mas as i32,
                                prev_amount: todo!(),
                                amount: coins,
                                post_amount: todo!(),
                                timestamp: todo!(),
                                slot: Some(value.slot.into()),
                                origin: grpc_model::CoinOrigin::AsyncMsg as i32,
                            }
                        };

                    vec![
                        common_fields(
                            todo!(),
                            msg.sender.unwrap().to_string(),
                            grpc_model::CoinDirection::Debit,
                        ),
                        common_fields(
                            todo!(),
                            msg.destination.unwrap().to_string(),
                            grpc_model::CoinDirection::Credit,
                        ),
                    ]
                }),
        );

        result.extend(
            value
                .deferred_calls_messages
                .into_iter()
                .filter_map(|res| res.ok())
                .flat_map(|def_call| {
                    let common_fields =
                        |id: String,
                         address: String,
                         amount: Amount,
                         direction: grpc_model::CoinDirection| {
                            grpc_model::ExecutionInfo {
                                id,
                                address,
                                direction: direction as i32,
                                item: grpc_model::Item::Mas as i32,
                                prev_amount: todo!(),
                                amount: Some(amount.into()),
                                post_amount: todo!(),
                                timestamp: todo!(),
                                slot: Some(value.slot.into()),
                                origin: grpc_model::CoinOrigin::DeferredCall as i32,
                            }
                        };

                    vec![
                        common_fields(
                            todo!(),
                            def_call.sender.to_string(),
                            def_call.coins,
                            grpc_model::CoinDirection::Debit,
                        ),
                        common_fields(
                            todo!(),
                            def_call.sender.to_string(),
                            def_call.fee,
                            grpc_model::CoinDirection::Debit,
                        ),
                        common_fields(
                            todo!(),
                            def_call.target_address.to_string(),
                            def_call.coins,
                            grpc_model::CoinDirection::Credit,
                        ),
                    ]
                }),
        );

        result.extend(
            value
                .deferred_credits_execution
                .into_iter()
                .filter_map(|(addr, res)| res.ok().map(|amount| (addr, amount)))
                .map(|(addr, amount)| grpc_model::ExecutionInfo {
                    id: todo!(),
                    address: addr.to_string(),
                    direction: grpc_model::CoinDirection::Credit as i32,
                    item: grpc_model::Item::Mas as i32,
                    prev_amount: todo!(),
                    amount: Some(amount.into()),
                    post_amount: todo!(),
                    timestamp: todo!(),
                    slot: Some(value.slot.into()),
                    origin: grpc_model::CoinOrigin::DeferredCredit as i32,
                }),
        );

        // TODO cancel_async_message_execution and auto_sell_execution

        // for msg in value.async_messages.into_iter() {
        //     match msg {
        //         Ok(msg) => {
        //             result.push(grpc_model::ExecutionInfo {
        //                 id: todo!(),
        //                 address: msg.sender.unwrap().to_string(),
        //                 direction: grpc_model::CoinDirection::Debit as i32,
        //                 item: grpc_model::Item::Mas as i32,
        //                 prev_amount: todo!(),
        //                 amount: Some(msg.coins.map(|a| a.into()).unwrap()),
        //                 post_amount: todo!(),
        //                 timestamp: todo!(),
        //                 slot: todo!(),
        //                 origin: grpc_model::CoinOrigin::AsyncMsg as i32,
        //             });
        //             result.push(grpc_model::ExecutionInfo {
        //                 id: todo!(),
        //                 address: msg.destination.unwrap().to_string(),
        //                 direction: grpc_model::CoinDirection::Credit as i32,
        //                 item: grpc_model::Item::Mas as i32,
        //                 prev_amount: todo!(),
        //                 amount: Some(msg.coins.map(|a| a.into()).unwrap()),
        //                 post_amount: todo!(),
        //                 timestamp: todo!(),
        //                 slot: todo!(),
        //                 origin: grpc_model::CoinOrigin::AsyncMsg as i32,
        //             });
        //         }
        //         Err(_) => {}
        //     }
        // }
        // grpc_api::NewExecutionInfoServerResponse {
        //     block_producer_reward: value.block_producer_reward.map(|(address, amount)| {
        //         grpc_model::TargetAmount {
        //             address: address.to_string(),
        //             amount: Some(amount.into()),
        //         }
        //     }),
        //     endorsement_creator_rewards: value
        //         .endorsement_creator_rewards
        //         .into_iter()
        //         .map(|(address, amount)| grpc_model::TargetAmount {
        //             address: address.to_string(),
        //             amount: Some(amount.into()),
        //         })
        //         .collect(),
        //     endorsement_target_reward: value.endorsement_target_reward.map(|(address, amount)| {
        //         grpc_model::TargetAmount {
        //             address: address.to_string(),
        //             amount: Some(amount.into()),
        //         }
        //     }),
        //     denunciations: value
        //         .denunciations
        //         .into_iter()
        //         .filter_map(|result| match result {
        //             Ok(den) => Some(grpc_model::DenunciationAddress {
        //                 address_denounced: den.address_denounced.to_string(),
        //                 slot: Some(den.slot.into()),
        //                 slashed: Some(den.slashed.into()),
        //             }),
        //             Err(_) => None,
        //         })
        //         .collect(),
        //     async_messages: value
        //         .async_messages
        //         .iter()
        //         .filter_map(|r| match r {
        //             Ok(msg) => Some(grpc_model::AsyncMessageExecution {
        //                 success: msg.success,
        //                 sender: msg.sender.unwrap().to_string(),
        //                 destination: msg.destination.unwrap().to_string(),
        //                 coins: msg.coins.map(|a| a.into()),
        //             }),
        //             Err(_) => None,
        //         })
        //         .collect(),
        //     operations: value
        //         .operations
        //         .into_iter()
        //         .map(|op| match op {
        //             crate::execution_info::OperationInfo::RollBuy(addr, amount) => {
        //                 grpc_model::OperationTypeRoll {
        //                     address: addr.to_string(),
        //                     r#type: Some(grpc_model::operation_type_roll::Type::RollBuy(
        //                         grpc_model::RollBuy { roll_count: amount },
        //                     )),
        //                 }
        //             }
        //             crate::execution_info::OperationInfo::RollSell(addr, amount) => {
        //                 grpc_model::OperationTypeRoll {
        //                     address: addr.to_string(),
        //                     r#type: Some(grpc_model::operation_type_roll::Type::RollSell(
        //                         grpc_model::RollSell { roll_count: amount },
        //                     )),
        //                 }
        //             }
        //         })
        //         .collect(),
        //     deferred_calls_messages: value
        //         .deferred_calls_messages
        //         .iter()
        //         .filter_map(|res| match res {
        //             Ok(def_call) => Some(grpc_model::DeferredCallExecution {
        //                 success: def_call.success,
        //                 sender: def_call.sender.to_string(),
        //                 target_address: def_call.target_address.to_string(),
        //                 target_function: def_call.target_function.to_string(),
        //                 coins: Some(def_call.coins.into()),
        //             }),
        //             Err(_) => None,
        //         })
        //         .collect(),
        //     deferred_credits_execution: value
        //         .deferred_credits_execution
        //         .into_iter()
        //         .filter_map(|(addr, res)| match res {
        //             Ok(amount) => Some(grpc_model::TargetAmount {
        //                 address: addr.to_string(),
        //                 amount: Some(amount.into()),
        //             }),
        //             Err(_) => None,
        //         })
        //         .collect(),
        //     cancel_async_message_execution: value
        //         .cancel_async_message_execution
        //         .into_iter()
        //         .filter_map(|(addr, res)| match res {
        //             Ok(amount) => Some(grpc_model::TargetAmount {
        //                 address: addr.to_string(),
        //                 amount: Some(amount.into()),
        //             }),
        //             Err(_) => None,
        //         })
        //         .collect(),
        //     auto_sell_execution: value
        //         .auto_sell_execution
        //         .into_iter()
        //         .map(|(addr, amount)| grpc_model::TargetAmount {
        //             address: addr.to_string(),
        //             amount: Some(amount.into()),
        //         })
        //         .collect(),
        // }

        grpc_api::NewExecutionInfoServerResponse {
            execution_infos: result,
        }
    }
}
