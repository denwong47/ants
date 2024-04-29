//! Simple to ``stderr`` logger.

/// Timestamp related functions.
///
/// This module is used by the generated macros in external crates; so this has
/// to be public in scope.
#[cfg(feature = "debug")]
pub mod timestamp {
    use chrono::Utc;

    /// Generate the current timestamp.
    pub fn now() -> String {
        Utc::now().to_rfc3339()
    }
}

include!(concat!(env!("OUT_DIR"), "/logger.rs"));
