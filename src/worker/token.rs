//! Generation of reservation tokens for workers.
//!

use crate::token;

/// Re-export the token generation function as reservation token generator.
pub use token::generate_token;

/// A reservation token.
pub type ReservationToken = token::MessageToken;

/// Timeout for a token.
pub const TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(15);
