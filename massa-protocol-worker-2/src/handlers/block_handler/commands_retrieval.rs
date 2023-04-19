use massa_models::{prehash::{PreHashMap, PreHashSet}, block_header::SecuredHeader, block_id::BlockId};

pub enum BlockHandlerRetrievalCommand {
    /// Wish list delta
    WishlistDelta {
        /// add to wish list
        new: PreHashMap<BlockId, Option<SecuredHeader>>,
        /// remove from wish list
        remove: PreHashSet<BlockId>,
    },
}