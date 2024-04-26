//! Model for System Messages.

use serde::{Deserialize, Serialize};
use std::io;
use uuid::Uuid;

use super::MulticastAgent;

use crate::{
    build_error, logger,
    proto::{multicast_message, MulticastMessage},
};

/// System Message
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SystemMessage {
    SelfTest(Uuid),
}

impl<T> MulticastAgent<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
{
    /// Send a self test message to the multicast group.
    pub async fn perform_self_test(&self) -> io::Result<()> {
        // If the agent is not listening, return an error.
        if !self.is_listening() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Agent is not listening.",
            ));
        }

        // Instantiate a watch channel to wait the SystemMessage to be returned.
        let (tx, mut rx) = tokio::sync::watch::channel(None);

        // If the watch channel is already set, return an error.
        self._self_test_received.read().await.set(tx).map_err(
            |_| io::Error::new(io::ErrorKind::ConnectionRefused, "Self-test watch channel is already set, cannot run two self-tests at the same time.")
        )?;

        // An inner scope that allows `Err` to be short-circuited to resetting the
        // watch channel.
        let validation = async {
            let secret = Uuid::new_v4();

            let message = MulticastMessage::new(
                &secret,
                multicast_message::Kind::System,
                serde_json::to_string(&SystemMessage::SelfTest(secret)).map_err(|err| {
                    build_error(&format!("Failed to serialize self-test message: {err}"))
                })?,
                Some(0),
            );

            // This HAS to be a loopback.
            self.send_message(&secret, message, true).await?;

            loop {
                rx.changed().await.map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::ConnectionRefused,
                        "Self-test watch channel was reset before the self-test was completed.",
                    )
                })?;

                if let Some(uuid) = rx.borrow_and_update().as_ref() {
                    if uuid == &secret {
                        logger::info!("Correct self-test message {} received.", secret);
                        break;
                    }
                }
            }

            Ok(())
        };

        // Do NOT use `?` here, as it will short-circuit the watch channel reset.
        let result = tokio::time::timeout(tokio::time::Duration::from_secs(5), validation)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Self-test timed out."))
            .and_then(
                // Flatten the result
                |result| result,
            );

        self._self_test_received
            .write()
            .await
            .take()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    "Self-test watch channel was reset before the self-test was completed.",
                )
            })?;

        result
    }

    /// Resolve a [`SystemMessage`].
    pub async fn resolve_system_message(&self, _message: SystemMessage) -> io::Result<()> {
        match _message {
            SystemMessage::SelfTest(uuid) => {
                if let Some(tx) = self._self_test_received.read().await.get() {
                    logger::info!("Received self-test message {}.", uuid);

                    tx.send(Some(uuid)).map_err(
                        |_| io::Error::new(io::ErrorKind::BrokenPipe, "Self-test watch channel was reset before the self-test was completed, could not send self-test message.")
                    )?;
                } else {
                    logger::debug!(
                        "Received self-test message {} when not expecting one, ignoring.",
                        uuid
                    );
                }

                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use serial_test::serial;

    use super::*;
    use crate::_tests::{TestStruct, MULTICAST_ADDRESS};
    use std::sync::Arc;

    #[tokio::test]
    #[serial]
    async fn test_send_self_test() {
        let agent = Arc::new(
            MulticastAgent::<TestStruct>::new(MULTICAST_ADDRESS)
                .expect("Failed to create multicast agent."),
        );

        let self_test_factory = || agent.perform_self_test();

        // Test the self test before the agent is listening.
        self_test_factory()
            .await
            .expect_err("Self-test should fail because the agent is not listening.");

        agent.start().await;

        // Test the self test.
        self_test_factory()
            .await
            .expect("Failed to perform self-test.");

        // Make sure the self-test is not running concurrently.
        let (result1, result2) = tokio::join!(self_test_factory(), self_test_factory());

        assert!(result1.is_ok() ^ result2.is_ok())
    }
}
