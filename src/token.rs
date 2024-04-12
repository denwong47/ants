//! Generation of message tokens for workers.
//!

use rand::Rng;

/// A reservation token.
pub type MessageToken = u64;

/// Generate a random message token.
pub fn generate_token() -> u64 {
    rand::thread_rng().gen_range(1..u64::MAX)
}
