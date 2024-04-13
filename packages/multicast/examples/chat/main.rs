//! A simple chat server that broadcasts messages to all connected clients.
//!
//! This uses the `multicast` crate to send and receive messages to a multicast group.
//! All messages are broadcast to all connected clients, which will then rebroadcast
//! the message to increase the reach and reliability of the message. Acknowledgements
//! will also be broadcasted, so that the entire network knows who has received
//! what message. This allow for uniform message delivery across the network should
//! we decide to.

use std::io;

use clap::Parser;

mod chatter;

mod config;
mod models;

mod printer;

#[tokio::main]
async fn main() -> io::Result<()> {
    let options = models::ChatOptions::parse();

    let multicast_addr = options.multicast_addr()?;

    chatter::new(
        chatter::User::local(options.name, options.colour),
        multicast_addr,
        options.packet_size,
        1,
    )?
    .start()
    .await
}
