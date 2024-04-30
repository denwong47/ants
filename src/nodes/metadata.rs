//! Node Metadata.
//!

/// Metadata for a node.
///
/// This is used to track the status of a node. Currently, it only tracks the
/// last time a node was used, but can be track its Epoch, error count, etc.
#[derive(Clone, Debug)]
pub struct NodeMetadata {
    last_used: tokio::time::Instant,
}

impl Default for NodeMetadata {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeMetadata {
    /// Create a new node metadata.
    pub fn new() -> Self {
        Self {
            last_used: tokio::time::Instant::now(),
        }
    }

    /// Create a new node metadata with a specific last used time.
    pub fn with_last_used(last_used: tokio::time::Instant) -> Self {
        Self { last_used }
    }

    /// Get the last time the node was used.
    pub fn last_used(&self) -> tokio::time::Instant {
        self.last_used
    }

    /// Update the last time the node was used.
    pub fn update_last_used(&mut self) {
        self.update_last_used_to(tokio::time::Instant::now());
    }

    /// Update the last time the node was used to the provided time.
    pub fn update_last_used_to(&mut self, last_used: tokio::time::Instant) {
        self.last_used = last_used;
    }
}
