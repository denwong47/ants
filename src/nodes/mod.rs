//! A struct to abstract the handling the node list.
//!
//! A node list needs to have the following properties:
//! - A O(1) way to get the next available least-used node,
//! - A O(1) way to check if a node exists in the list.
//! - A <O(n) way to insert and remove nodes.
//!
//! These requirements do not work well together, as the first calls for a
//! min heap, while the second and third call for a hash set.
//!
//! This struct composites both of these data structures to provide the
//! required functionality.

use core::cmp::Reverse;
use fxhash::FxHashMap;
use std::collections::BinaryHeap;
use tokio::sync::{Mutex, MutexGuard, RwLock, RwLockWriteGuard};

pub mod checkout;

mod metadata;
pub use metadata::*;

pub type NodeAddress = (String, u16);
pub type NodeRecord = (tokio::time::Instant, NodeAddress);

/// A list of nodes.
///
/// This struct composites a heap and a set to allow for O(1) access to the
/// next least-used node, and O(1) access to check if a node exists in the list.
///
/// The heap is used to store the nodes in order of least-used to most-recently-used,
/// while the set is used to check if a node exists in the list.
///
/// The [`RwLock`] around the [`Self::set`] allows for concurrent reads and exclusive
/// writes.
#[derive(Debug)]
pub struct NodeList {
    heap: Mutex<BinaryHeap<Reverse<NodeRecord>>>,
    set: RwLock<FxHashMap<NodeAddress, NodeMetadata>>,
}

impl NodeList {
    /// Create a new node list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new node list with the provided [`NodeAddress`] instances.
    pub fn from_vec(nodes: Vec<NodeAddress>) -> Self {
        let time = tokio::time::Instant::now();
        let heap = BinaryHeap::from_iter(nodes.iter().map(|node| Reverse((time, node.clone()))));
        let set = FxHashMap::from_iter(
            nodes
                .into_iter()
                .map(|node| (node, NodeMetadata::with_last_used(time))),
        );

        Self {
            heap: Mutex::new(heap),
            set: RwLock::new(set),
        }
    }

    /// Check if a node exists in the list.
    ///
    /// # Note
    ///
    /// It is not required to call this method before calling
    /// [`Self::insert`]; it is provided for pure checks only.
    pub async fn contains(&self, node: &NodeAddress) -> bool {
        self.set.read().await.contains_key(node)
    }

    /// Lock both the heap and the set for writing at once, to prevent
    /// the heap from getting out of sync with the set.
    async fn lock_for_write(
        &self,
    ) -> (
        MutexGuard<BinaryHeap<Reverse<NodeRecord>>>,
        RwLockWriteGuard<FxHashMap<NodeAddress, NodeMetadata>>,
    ) {
        (self.heap.lock().await, self.set.write().await)
    }

    /// Insert a node into the list.
    ///
    /// Returns if the node was inserted.
    pub async fn insert(&self, node: NodeAddress) -> bool {
        if !self.contains(&node).await {
            let (mut heap, mut set) = self.lock_for_write().await;

            let time = tokio::time::Instant::now();
            let record = (time, node.clone());

            heap.push(Reverse(record));
            set.insert(node.clone(), NodeMetadata::with_last_used(time))
                .is_none()
        } else {
            false
        }
    }

    /// Insert a node by metadata into the list.
    ///
    /// This will automatically update the last used time of the node.
    pub async fn insert_node(&self, node: NodeAddress, mut metadata: NodeMetadata) -> bool {
        let (mut heap, mut set) = self.lock_for_write().await;

        let time = tokio::time::Instant::now();
        let record = (time, node.clone());
        metadata.update_last_used_to(time);

        heap.push(Reverse(record));
        set.insert(node.clone(), metadata).is_none()
    }

    /// Remove a node from the list.
    ///
    /// This does not actually remove the node from the [Self::heap], due to the cost
    /// of element removal from heaps. Instead, it marks the node as removed in the
    /// [Self::set], and when we pop the heap, we discard any nodes that are marked
    /// as such.
    pub async fn remove(&self, node: &NodeAddress) {
        if self.contains(node).await {
            self.set.write().await.remove(node);
        }
    }

    /// Get the next least-used node, along with the duration since it was last used.
    pub async fn next_with_metadata(&self) -> Option<(NodeMetadata, NodeAddress)> {
        let (mut heap, mut set) = self.lock_for_write().await;

        while let Some(Reverse((time, node))) = heap.pop() {
            // Check if the node is still in the set first. If not, then discard it.
            if let Some(metadata) = set.get(&node) {
                if time >= metadata.last_used() {
                    let metadata_opt = set.remove(&node);

                    if let Some(metadata) = metadata_opt {
                        return Some((metadata, node));
                    }
                }
                // If the heap node was found to be older than the last inserted time in
                // the set, then we know that the node was at some point removed from the
                // set and re-inserted, upon which a duplicate record will be present in
                // the heap. What we got at this point from the heap is out of date, and
                // we should discard it.
            }
        }

        None
    }

    /// Get the next least-used node.
    pub async fn next(&self) -> Option<NodeAddress> {
        self.next_with_metadata().await.map(|(_, node)| node)
    }

    /// Get a random node from the list.
    ///
    /// This will only remove the node from the set, but not the heap. This is because
    /// [`Self::next_with_metadata`] will discard any nodes that are not in the set. As
    /// long as the node is re-inserted into the set with a later timestamp, the heap
    /// entry for this node will be discarded.
    pub async fn random_with_metadata(&self) -> Option<(NodeMetadata, NodeAddress)> {
        let (_, mut set) = self.lock_for_write().await;

        if set.is_empty() {
            return None;
        }

        let index = rand::random::<usize>() % set.len();
        let node = set.keys().nth(index).unwrap().clone();

        set.remove(&node).map(|metadata| (metadata, node.clone()))
    }

    /// Get a random node from the list.
    ///
    /// This is a convenience method that only returns the node address; see
    /// [`Self::random_with_metadata`] for more control.
    pub async fn random(&self) -> Option<NodeAddress> {
        self.random_with_metadata().await.map(|(_, node)| node)
    }

    /// Get the number of nodes in the list.
    pub async fn len(&self) -> usize {
        self.set.read().await.len()
    }

    /// Return if the list is empty.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Get the age of a specific node in the list.
    pub async fn last_used(&self, node: &NodeAddress) -> Option<tokio::time::Instant> {
        self.set
            .read()
            .await
            .get(node)
            .map(|metadata| metadata.last_used())
    }

    /// Get the age of the least-used node.
    pub async fn last_used_for_next(&self) -> Option<tokio::time::Instant> {
        self.heap
            .lock()
            .await
            .peek()
            .map(|Reverse((time, _))| *time)
    }
}

impl Default for NodeList {
    /// Create a new empty node list.
    fn default() -> Self {
        Self {
            heap: Mutex::new(BinaryHeap::new()),
            set: RwLock::new(FxHashMap::default()),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    pub(super) const NODES: [(&str, u16); 3] =
        [("1.1.1.1", 1234), ("2.2.2.2", 2345), ("3.3.3.3", 3456)];

    #[tokio::test]
    async fn node_list() {
        let nodes = NODES
            .iter()
            .map(|(host, port)| (host.to_string(), *port))
            .collect::<Vec<_>>();

        let list = NodeList::new();

        // Check if the list is empty.
        assert_eq!(list.len().await, 0);
        assert!(!list.contains(&nodes[0]).await);

        // Insert all the nodes.
        for node in &nodes {
            assert!(list.insert(node.clone()).await);
            // We must sleep here to ensure that the time is different for each node.
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

            let age = list.last_used(node).await.unwrap();
            // Try inserting the same node again...
            list.insert(node.clone()).await;

            // ...but the age should NOT have changed.
            assert_eq!(list.last_used(node).await.unwrap(), age);
        }

        assert_eq!(list.len().await, 3);

        // Insert the same node again.
        for node in &nodes {
            assert!(!list.insert(node.clone()).await);
        }

        assert_eq!(list.len().await, 3);

        // Remove a node.
        list.remove(&nodes[0]).await;
        assert_eq!(list.len().await, 2);
        assert!(!list.contains(&nodes[0]).await);

        // Re-insert the node.
        assert!(list.insert(nodes[0].clone()).await);

        // Drain the list. The ordering is important here.
        // This is also quite an important test for re-insertion: the nodes[0]
        // was removed and re-inserted before its equivalent record in the heap
        // was popped; so this tests if `next` correctly discards the old record.
        assert_eq!(list.next().await.as_ref(), Some(&nodes[1]));
        assert_eq!(list.next().await.as_ref(), Some(&nodes[2]));
        assert_eq!(list.next().await.as_ref(), Some(&nodes[0]));
    }
}
