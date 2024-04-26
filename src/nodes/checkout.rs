//! A temporary struct to hold a [`NodeAddress`] being checked out for reservation.
//!
//! Upon [`Drop`], the node is automatically added back to the list of available nodes.

use super::{NodeAddress, NodeList};
use std::sync::{Arc, OnceLock};

pub struct CheckedOutNode {
    node: OnceLock<NodeAddress>,
    pub since_last_used: tokio::time::Duration,
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
    pub async fn complete(mut self) -> NodeAddress {
        let node = self
            .node
            .take()
            .expect("Checked out node was consumed before being used.");

        self.list.insert(node.clone()).await;
        node
    }

    /// Discard the checked out node, and add it back to the list of available nodes.
    ///
    /// Similar to [`Self::complete`], but this does not return the node.
    pub async fn discard(mut self) {
        let node = self
            .node
            .take()
            .expect("Checked out node was consumed before being used.");

        self.list.insert(node).await;
    }
}

impl Drop for CheckedOutNode {
    /// Add the node back to the list of available nodes.
    ///
    /// This is called when the checked out node is dropped; and this expects a
    /// [`tokio::runtime::Handle`] to be available.
    fn drop(&mut self) {
        if let Some(node) = self.node.take() {
            tokio::spawn({
                let list = Arc::clone(&self.list);
                async move {
                    list.insert(node).await;
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
        let (since_last_used, node) = self.next_with_duration().await?;

        Some(CheckedOutNode {
            node: OnceLock::from(node),
            since_last_used,
            list: Arc::clone(self),
        })
    }
}

#[cfg(test)]
mod test {
    use super::super::test::NODES;
    use super::*;

    #[tokio::test]
    async fn checked_out_node() {
        let nodes = test::NODES
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

        // Checkout the second node, and use the `discard` method to add it back to the list.
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
}
