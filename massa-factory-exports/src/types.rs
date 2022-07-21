use massa_models::Block;

/// History of block production from latest to oldest
/// todo: redesign type (maybe add slots, draws...)
pub type ProductionHistory = Vec<Block>;
