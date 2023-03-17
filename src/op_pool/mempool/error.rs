use ethers::{abi::Address, types::U256};
use parse_display::Display;

/// Mempool result type.
pub type MempoolResult<T> = std::result::Result<T, MempoolError>;

/// Mempool error type.
#[derive(Debug, thiserror::Error)]
pub enum MempoolError {
    /// Operation with same sender/nonce already in pool
    /// and the replacement operation has lower gas price.
    #[error("Replacement operation underpriced. Existing priority fee: {0}. Existing fee: {1}")]
    ReplacementUnderpriced(U256, U256),
    /// Max operations reached for this sender
    #[error("Max operations ({0}) reached for sender {1}")]
    MaxOperationsReached(usize, Address),
    /// An entity associated with the operation is throttled/banned.
    #[error("Entity {0}:{1} is throttled/banned")]
    EntityThrottled(EntityType, Address),
}

#[derive(Display, Debug, Clone, Copy)]
#[display(style = "lowercase")]
pub enum EntityType {
    Sender,
    Paymaster,
    Aggregator,
    Factory,
}