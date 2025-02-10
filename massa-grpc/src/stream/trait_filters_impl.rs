use std::collections::HashSet;
use std::str::FromStr;

use crate::SlotRange;
use crate::{config::GrpcConfig, error::GrpcError};
use massa_execution_exports::{ExecutionOutput, SlotExecutionOutput};
use massa_models::address::Address;
use massa_models::block::{FilledBlock, SecureShareBlock};
use massa_models::block_id::BlockId;
use massa_models::operation::{OperationId, SecureShareOperation};
use massa_models::slot::Slot;
use massa_proto_rs::massa::api::v1::{
    self as grpc_api, NewBlocksRequest, NewFilledBlocksRequest, NewOperationsRequest,
    NewSlotExecutionOutputsRequest,
};
use massa_proto_rs::massa::model::v1::{self as grpc_model};

/// Trait implementation for filtering the output based on the request
pub(crate) trait FilterGrpc<RequestType, FilterType, Data> {
    /// Build the filter from the request
    fn build_from_request(
        request: RequestType,
        grpc_config: &GrpcConfig,
    ) -> Result<FilterType, GrpcError>;
    /// Filter the output based on the filter
    fn filter_output(&self, content: Data, grpc_config: &GrpcConfig) -> Option<Data>;
}

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

// Type declaration for NewOperationsFilter
#[derive(Debug)]
pub(crate) struct FilterNewOperations {
    // Operation ids to filter
    operation_ids: Option<HashSet<OperationId>>,
    // Addresses to filter
    addresses: Option<HashSet<Address>>,
    // Operation types to filter
    operation_types: Option<HashSet<i32>>,
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

// Type declaration for NewFilledBlocksFilter
#[derive(Clone, Debug)]
pub(crate) struct FilterNewFilledBlocks {
    // Block ids to filter
    block_ids: Option<HashSet<BlockId>>,
    // Addresses to filter
    addresses: Option<HashSet<Address>>,
    // Slot range to filter
    slot_ranges: Option<HashSet<SlotRange>>,
}

impl FilterGrpc<NewSlotExecutionOutputsRequest, FilterNewSlotExec, SlotExecutionOutput>
    for FilterNewSlotExec
{
    fn build_from_request(
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

    fn filter_output(
        &self,
        content: SlotExecutionOutput,
        grpc_config: &GrpcConfig,
    ) -> Option<SlotExecutionOutput> {
        match content {
            SlotExecutionOutput::ExecutedSlot(e_output) => filter_map_exec_output_inner(
                e_output,
                self,
                grpc_config,
                grpc_model::ExecutionOutputStatus::Candidate as i32,
            )
            .map(SlotExecutionOutput::ExecutedSlot),
            SlotExecutionOutput::FinalizedSlot(e_output) => filter_map_exec_output_inner(
                e_output,
                self,
                grpc_config,
                grpc_model::ExecutionOutputStatus::Final as i32,
            )
            .map(SlotExecutionOutput::FinalizedSlot),
        }
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

impl FilterGrpc<NewBlocksRequest, FilterNewBlocks, SecureShareBlock> for FilterNewBlocks {
    fn build_from_request(
        request: NewBlocksRequest,
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
                                GrpcError::InvalidArgument(format!(
                                    "invalid block id: {}",
                                    block_id
                                ))
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

    fn filter_output(
        &self,
        content: SecureShareBlock,
        grpc_config: &GrpcConfig,
    ) -> Option<SecureShareBlock> {
        if let Some(block_ids) = &self.block_ids {
            if !block_ids.contains(&content.id) {
                return None;
            }
        }

        if let Some(addresses) = &self.addresses {
            if !addresses.contains(&content.content_creator_address) {
                return None;
            }
        }

        if let Some(slot_ranges) = &self.slot_ranges {
            let mut start_slot = Slot::new(0, 0); // inclusive
            let mut end_slot = Slot::new(u64::MAX, grpc_config.thread_count - 1); // exclusive

            for slot_range in slot_ranges {
                start_slot =
                    start_slot.max(slot_range.start_slot.unwrap_or_else(|| Slot::new(0, 0)));
                end_slot = end_slot.min(
                    slot_range
                        .end_slot
                        .unwrap_or_else(|| Slot::new(u64::MAX, grpc_config.thread_count - 1)),
                );
            }
            end_slot = end_slot.max(start_slot);
            let current_slot = content.content.header.content.slot;

            if !(current_slot >= start_slot && current_slot < end_slot) {
                return None;
            }
        }

        Some(content)
    }
}

impl FilterGrpc<NewOperationsRequest, FilterNewOperations, SecureShareOperation>
    for FilterNewOperations
{
    fn build_from_request(
        request: NewOperationsRequest,
        grpc_config: &GrpcConfig,
    ) -> Result<FilterNewOperations, GrpcError> {
        if request.filters.len() as u32 > grpc_config.max_filters_per_request {
            return Err(GrpcError::InvalidArgument(format!(
                "too many filters received. Only a maximum of {} filters are accepted per request",
                grpc_config.max_filters_per_request
            )));
        }

        let mut operation_ids_filter: Option<HashSet<OperationId>> = None;
        let mut addresses_filter: Option<HashSet<Address>> = None;
        let mut operation_types_filter: Option<HashSet<i32>> = None;

        // Get params filter from the request.
        for query in request.filters.into_iter() {
            if let Some(filter) = query.filter {
                match filter {
                    grpc_api::new_operations_filter::Filter::OperationIds(ids) => {
                        if ids.operation_ids.len() as u32
                            > grpc_config.max_operation_ids_per_request
                        {
                            return Err(GrpcError::InvalidArgument(format!(
                                "too many operation ids received. Only a maximum of {} operation ids are accepted per request",
                             grpc_config.max_block_ids_per_request
                            )));
                        }
                        let operation_ids = operation_ids_filter.get_or_insert_with(HashSet::new);
                        for id in ids.operation_ids {
                            operation_ids.insert(OperationId::from_str(&id).map_err(|_| {
                                GrpcError::InvalidArgument(format!("invalid operation id: {}", id))
                            })?);
                        }
                    }
                    grpc_api::new_operations_filter::Filter::Addresses(addrs) => {
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
                    grpc_api::new_operations_filter::Filter::OperationTypes(ope_types) => {
                        // The length limited to the number of operation types in the enum
                        if ope_types.op_types.len() as u64 > 6 {
                            return Err(GrpcError::InvalidArgument(
                                "too many operation types received. Only a maximum of 6 operation types are accepted per request".to_string()
                            ));
                        }
                        let operation_types =
                            operation_types_filter.get_or_insert_with(HashSet::new);
                        operation_types.extend(&ope_types.op_types);
                    }
                }
            }
        }

        Ok(FilterNewOperations {
            operation_ids: operation_ids_filter,
            addresses: addresses_filter,
            operation_types: operation_types_filter,
        })
    }

    fn filter_output(
        &self,
        content: SecureShareOperation,
        _grpc_config: &GrpcConfig,
    ) -> Option<SecureShareOperation> {
        if let Some(operation_ids) = &self.operation_ids {
            if !operation_ids.contains(&content.id) {
                return None;
            }
        }

        if let Some(addresses) = &self.addresses {
            if !addresses.contains(&content.content_creator_address) {
                return None;
            }
        }

        if let Some(operation_types) = &self.operation_types {
            let op_type = grpc_model::OpType::from(content.content.op.clone()) as i32;
            if !operation_types.contains(&op_type) {
                return None;
            }
        }

        Some(content)
    }
}

impl FilterGrpc<NewFilledBlocksRequest, FilterNewFilledBlocks, FilledBlock>
    for FilterNewFilledBlocks
{
    fn build_from_request(
        request: NewFilledBlocksRequest,
        grpc_config: &GrpcConfig,
    ) -> Result<FilterNewFilledBlocks, GrpcError> {
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
                                GrpcError::InvalidArgument(format!(
                                    "invalid block id: {}",
                                    block_id
                                ))
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

        Ok(FilterNewFilledBlocks {
            block_ids: block_ids_filter,
            addresses: addresses_filter,
            slot_ranges: slot_ranges_filter,
        })
    }

    fn filter_output(&self, content: FilledBlock, grpc_config: &GrpcConfig) -> Option<FilledBlock> {
        if let Some(block_ids) = &self.block_ids {
            if !block_ids.contains(&content.header.id) {
                return None;
            }
        }

        if let Some(addresses) = &self.addresses {
            if !addresses.contains(&content.header.content_creator_address) {
                return None;
            }
        }

        if let Some(slot_ranges) = &self.slot_ranges {
            let mut start_slot = Slot::new(0, 0); // inclusive
            let mut end_slot = Slot::new(u64::MAX, grpc_config.thread_count - 1); // exclusive

            for slot_range in slot_ranges {
                start_slot =
                    start_slot.max(slot_range.start_slot.unwrap_or_else(|| Slot::new(0, 0)));
                end_slot = end_slot.min(
                    slot_range
                        .end_slot
                        .unwrap_or_else(|| Slot::new(u64::MAX, grpc_config.thread_count - 1)),
                );
            }
            end_slot = end_slot.max(start_slot);
            let current_slot = content.header.content.slot;

            if !(current_slot >= start_slot && current_slot < end_slot) {
                return None;
            }
        }

        Some(content)
    }
}
