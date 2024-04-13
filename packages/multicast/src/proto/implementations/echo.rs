//! Check the TTL of the packet and if it is not 0, decrement it and return
//! the echo version of the packet.
//!

use super::super::{multicast_message, MulticastMessage};

impl MulticastMessage {
    /// Check the TTL of the packet and if it is not 0, decrement it and return
    /// the echo version of the packet.
    pub fn echo(&self) -> Option<MulticastMessage> {
        if self.ttl == 0 {
            return None;
        }

        let mut echo = self.clone();
        echo.kind = multicast_message::Kind::Echo.into();
        echo.ttl -= 1;
        Some(echo)
    }
}
