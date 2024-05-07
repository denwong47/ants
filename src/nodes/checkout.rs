//! A temporary struct to hold a [`NodeAddress`] being checked out for reservation.
//!
//! Upon [`Drop`], the node is automatically added back to the list of available nodes.

use super::{NodeAddress, NodeList, NodeMetadata};
use core::panic;
use std::sync::{Arc, OnceLock};

#[derive(Debug)]
pub struct CheckedOutNode {
    node: OnceLock<NodeAddress>,
    pub metadata: NodeMetadata,
    list: Arc<NodeList>,
}

impl CheckedOutNode {
    /// Get the checked out node.
    pub fn address(&self) -> &NodeAddress {
        self.node
            .get()
            .expect("Checked out node was consumed before being used.")
    }

    /// Get the host of the checked out node.
    pub fn host(&self) -> &str {
        &self.address().0
    }

    /// Get the port of the checked out node.
    pub fn port(&self) -> u16 {
        self.address().1
    }

    /// Get the [`tokio::time::Instant`] when the node was last used.
    pub fn last_used(&self) -> tokio::time::Instant {
        self.metadata.last_used()
    }

    /// Get the [`tokio::time::Duration`] since the node was last used.
    pub fn since_last_used(&self) -> tokio::time::Duration {
        self.last_used().elapsed()
    }

    /// Consume the checked out node, and not add it back to the list of available nodes.
    pub fn consume(mut self) -> NodeAddress {
        self.node
            .take()
            .expect("Checked out node was consumed before being used.")
    }

    /// Return the checked out node, and add a copy of it back to the list of available nodes.
    ///
    /// This is an async function, and requires a [`tokio::runtime::Handle`] to be available.
    /// It is generally recommended to use this over the [`Drop`] implementation, as this
    /// does not wait for a spawned task to complete; which may result in the node not
    /// being re-inserted until the next `await` call.
    pub async fn complete(self) -> NodeAddress {
        let node = self
            .node
            .get()
            .cloned()
            .unwrap_or_else(|| panic!("Checked out node was consumed before being used."));

        let list = Arc::clone(&self.list);
        list.reinsert_checked_out(self).await;
        node
    }

    /// Discard the checked out node, and add it back to the list of available nodes.
    ///
    /// Similar to [`Self::complete`], but this does not return the node.
    pub async fn discard(self) {
        let list = Arc::clone(&self.list);
        list.reinsert_checked_out(self).await;
    }

    /// Decompose the checked out node into its parts.
    pub fn decompose(mut self) -> (NodeAddress, NodeMetadata) {
        let node = self
            .node
            .take()
            .expect("Checked out node was consumed before being used.");
        let metadata = self.metadata.clone();

        (node, metadata)
    }
}

impl Drop for CheckedOutNode {
    /// Add the node back to the list of available nodes.
    ///
    /// This is called when the checked out node is dropped; and this expects a
    /// [`tokio::runtime::Runtime`] to be available.
    fn drop(&mut self) {
        let metadata = self.metadata.clone();
        if let Some(node) = self.node.take() {
            tokio::spawn({
                let list = Arc::clone(&self.list);
                async move {
                    list.insert_node(node, metadata).await;
                }
            });
        }
    }
}

impl std::ops::Deref for CheckedOutNode {
    type Target = NodeAddress;

    /// Dereference into the checked out node.
    fn deref(&self) -> &Self::Target {
        self.address()
    }
}

impl NodeList {
    /// Check out the next least-used node.
    ///
    /// This can only be used if the [`NodeList`] is referenced as an [`Arc`].
    pub async fn checkout(self: &Arc<Self>) -> Option<CheckedOutNode> {
        let (metadata, node) = self.next_with_metadata().await?;

        Some(CheckedOutNode {
            node: OnceLock::from(node),
            metadata,
            list: Arc::clone(self),
        })
    }

    /// Check out a random node.
    ///
    /// This can only be used if the [`NodeList`] is referenced as an [`Arc`].
    pub async fn checkout_random(self: &Arc<Self>) -> Option<CheckedOutNode> {
        let (metadata, node) = self.random_with_metadata().await?;

        Some(CheckedOutNode {
            node: OnceLock::from(node),
            metadata,
            list: Arc::clone(self),
        })
    }

    /// Re-insert a checked out node back into the list.
    ///
    /// # Panics
    ///
    /// If the checked out node was consumed before being used - i.e. the node
    /// no longer has an address.
    pub async fn reinsert_checked_out(&self, node: CheckedOutNode) {
        let (node, metadata) = node.decompose();

        self.insert_node(node, metadata).await;
    }
}

#[cfg(test)]
mod test {
    use serial_test::serial;

    use crate::{Worker, _tests::*, config};

    use super::super::test::NODES;
    use super::*;

    #[tokio::test]
    async fn checked_out_node() {
        let nodes: Vec<(String, u16)> = test::NODES
            .iter()
            .map(|(host, port)| (host.to_string(), *port))
            .collect::<Vec<_>>();

        let list = Arc::new(NodeList::from_vec(nodes.clone()));

        let inserted_0 = list
            .last_used(&nodes[0])
            .await
            .expect("Node 0 was not inserted into the list.");

        // Checkout the first node, and use the `Drop` implementation to add it back to the list.
        '_checkout_0: {
            let node = list.checkout().await.unwrap();

            assert_eq!(list.len().await, nodes.len() - 1);

            assert_eq!(node.address(), &nodes[0]);

            // The node should not be in the list.
            assert!(list.last_used(&nodes[0]).await.is_none());
        }

        // This is to ensure that the task spawned by the `CheckedOutNode::drop` is completed.
        tokio::task::yield_now().await;

        // The node should be added back to the list.
        assert_eq!(list.len().await, nodes.len());

        assert_ne!(
            list.last_used(&nodes[0])
                .await
                .expect("Node 0 was not inserted back into the list."),
            inserted_0,
        );

        // =============================================================================

        let inserted_1 = list
            .last_used(&nodes[1])
            .await
            .expect("Node 1 was not inserted into the list.");

        // Checkout the second node, and use the `complete` method to add it back to the list.
        '_checkout_1: {
            let node = list.checkout().await.unwrap();

            assert_eq!(list.len().await, nodes.len() - 1);

            assert_eq!(node.address(), &nodes[1]);

            // The node should not be in the list.
            assert!(list.last_used(&nodes[1]).await.is_none());

            node.complete().await;
        }

        // The node should be added back to the list without running `yield_now`.
        assert_eq!(list.len().await, nodes.len());

        assert_ne!(
            list.last_used(&nodes[1])
                .await
                .expect("Node 1 was not inserted back into the list."),
            inserted_1,
        );

        // =============================================================================

        // Checkout the third node, and use the `consume` method to consume it.
        let consumed = '_checkout_2: {
            let node = list.checkout().await.unwrap();

            assert_eq!(list.len().await, nodes.len() - 1);

            assert_eq!(node.address(), &nodes[2]);

            // The node should not be in the list.
            assert!(list.last_used(&nodes[2]).await.is_none());

            node.consume()
        };

        // The node should not be added back to the list.
        assert_eq!(list.len().await, nodes.len() - 1);

        assert_eq!(consumed, nodes[2]);

        // =============================================================================

        let next = list.next().await.expect("No nodes left in the list.");

        // The first node should be back to the top.
        assert_eq!(next, nodes[0]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    #[serial]
    async fn check_out_random_node() {
        const WORKER_COUNT: usize = 5;

        let workers = Worker::new_and_init_multiple(
            WORKER_COUNT,
            BASE_HOST.to_owned(),
            BASE_PORT,
            vec![],
            config::DEFAULT_MULTICAST_HOST.to_owned(),
            config::DEFAULT_MULTICAST_PORT,
            pow_2,
            WORK_TIMEOUT,
        )
        .await
        .expect("Failed to create workers.");

        tokio::time::sleep(WAIT_FOR_WORKER).await;

        let inner_worker = Arc::clone(workers.first().unwrap());

        assert_eq!(inner_worker.nodes.len().await, WORKER_COUNT - 1); // Do not count itself.

        {
            let ordered_checkout = inner_worker
                .nodes
                .checkout()
                .await
                .expect("Failed to checkout ordered node.");
            let random_checkout = inner_worker
                .nodes
                .checkout_random()
                .await
                .expect("Failed to checkout random node.");

            assert!(ordered_checkout.address() != random_checkout.address());

            assert_eq!(inner_worker.nodes.len().await, WORKER_COUNT - 3); // Do not count itself.

            // Make sure the random node does not come up again.
            let mut count = 0;
            while let Some(remaining) = inner_worker.nodes.checkout().await {
                count += 1;
                assert!(![ordered_checkout.address(), random_checkout.address()]
                    .contains(&remaining.address()))
            }

            assert_eq!(count, WORKER_COUNT - 3);
        }

        // This is to ensure that the task spawned by the `CheckedOutNode::drop` is completed.
        tokio::time::sleep(WAIT_FOR_WORKER).await;

        assert_eq!(inner_worker.nodes.len().await, WORKER_COUNT - 1); // Do not count itself.

        // Teardown.
        logger::debug!("Waiting for the spawned tasks to complete.");

        for worker in workers.into_iter() {
            worker.teardown();
        }
    }
}
