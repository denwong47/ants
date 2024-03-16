//! Generation of reservation tokens for workers.
//!

use rand::Rng;

/// A reservation token.
pub type ReservationToken = u64;

/// Timeout for a token.
pub const TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(15);

/// Generate a random reservation token.
pub fn generate_token() -> u64 {
    rand::thread_rng().gen_range(1..u64::MAX)
}
