use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
};

pub enum BlockHandlerRetrievalCommand {
    /// Wish list delta
    WishlistDelta {
        /// add to wish list
        new: PreHashMap<BlockId, Option<SecuredHeader>>,
        /// remove from wish list
        remove: PreHashSet<BlockId>,
    },
}
