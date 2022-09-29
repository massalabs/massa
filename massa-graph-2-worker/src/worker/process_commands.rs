use massa_graph::error::GraphResult;

use super::GraphWorker;

impl GraphWorker {
    // pub fn incoming_header(
    //     &mut self,
    //     block_id: BlockId,
    //     header: WrappedHeader,
    //     current_slot: Option<Slot>,
    // ) -> GraphResult<()> {
    //     // ignore genesis blocks
    //     if self.genesis_hashes.contains(&block_id) {
    //         return Ok(());
    //     }

    //     debug!(
    //         "received header {} for slot {}",
    //         block_id, header.content.slot
    //     );
    //     massa_trace!("consensus.block_graph.incoming_header", {"block_id": block_id, "header": header});
    //     let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
    //     match self.block_statuses.entry(block_id) {
    //         // if absent => add as Incoming, call rec_ack on it
    //         hash_map::Entry::Vacant(vac) => {
    //             to_ack.insert((header.content.slot, block_id));
    //             vac.insert(BlockStatus::Incoming(HeaderOrBlock::Header(header)));
    //             self.incoming_index.insert(block_id);
    //         }
    //         hash_map::Entry::Occupied(mut occ) => match occ.get_mut() {
    //             BlockStatus::Discarded {
    //                 sequence_number, ..
    //             } => {
    //                 // promote if discarded
    //                 *sequence_number = BlockGraph::new_sequence_number(&mut self.sequence_counter);
    //             }
    //             BlockStatus::WaitingForDependencies { .. } => {
    //                 // promote in dependencies
    //                 self.promote_dep_tree(block_id)?;
    //             }
    //             _ => {}
    //         },
    //     }

    //     // process
    //     self.rec_process(to_ack, current_slot)?;

    //     Ok(())
    // }
}
