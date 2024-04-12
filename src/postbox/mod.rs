//! A postbox is a staging area where messages are stored before they are being delivered
//! to the recipient. Assume process A wants to send a message to process B, and expects
//! a reply in response. Since the messages are delivered asynchronously, process A needs
//! to await the reply without keeping the connection open. The postbox shall be the one
//! listening for any replies, perform any necessary processing and deduplications, then
//! notify anyone who is waiting for the reply.
//!

mod message;
pub use message::*;

mod model;
pub use model::*;
