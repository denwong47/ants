//! Chatter is a simple chat client that sends and receives messages to a multicast group.
//!

use std::{future::Future, io, net::SocketAddr, sync::Arc};

use super::{config, models::ChatDelivery, printer::Printer};
use fxhash::FxHashMap;
use multicast::{delivery::MulticastDelivery, logger, MulticastAgent};
use tokio::sync::RwLock;
use uuid::Uuid;

/// A user in the chat.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct User {
    pub identity: Uuid,
    pub name: String,
    pub colour: u8,
    pub discovered_from: SocketAddr,
    pub discovered_at: tokio::time::Instant,
}

impl User {
    /// Set up a local user.
    pub fn local(name: impl ToString, colour: u8) -> Self {
        User {
            identity: Uuid::new_v4(),
            name: name.to_string(),
            colour,
            discovered_from: multicast::LOOPBACK_ADDRESS,
            discovered_at: tokio::time::Instant::now(),
        }
    }

    /// Set up a remote user.
    pub fn remote(
        identity: Uuid,
        name: impl ToString,
        colour: u8,
        discovered_from: SocketAddr,
    ) -> Self {
        User {
            identity,
            name: name.to_string(),
            colour,
            discovered_from,
            discovered_at: tokio::time::Instant::now(),
        }
    }
}

/// A simple chat client that sends and receives messages to a multicast group.
pub struct Chatter<F>
where
    F: Future<Output = String> + Send + Sync + 'static,
{
    this_user: User,
    agent: Arc<MulticastAgent<ChatDelivery>>,
    printer: Arc<Printer<F>>,
    users: RwLock<FxHashMap<Uuid, User>>,
}

pub fn new(
    user: User,
    multicast_addr: SocketAddr,
    packet_size: usize,
    ttl: u32,
) -> io::Result<Arc<Chatter<impl Future<Output = String>>>> {
    let agent = Arc::new(
        MulticastAgent::<ChatDelivery>::new(multicast_addr)?
            .with_packet_size(packet_size)
            .with_ttl(ttl),
    );

    let uuid = user.identity;
    let printer_agent = agent.clone();
    let printer = Printer::new(
        "\u{2501}".repeat(60) + "\n\x1b[1G" + "\u{1F4AC} Enter your message: ",
        move |_printer, message| {
            let local_agent = printer_agent.clone();
            async move {
                local_agent
                    .send(ChatDelivery::Message {
                        identity: uuid,
                        message: message.clone(),
                    })
                    .await
                    .expect("Failed to send message to the multicast group.");

                format!(
                    "\x1b[F\x1b[2K\x1b[F\x1b[2K\x1b[1G\
                    \x1b[1m\x1b[38;5;{colour}mYou\x1b[39m:\x1b[22m\n\x1b[4G\x1b[38;5;8m{message}\x1b[39m",
                    colour = config::SELF_COLOUR,
                    message = message
                )
            }
        },
    );

    let mut users = FxHashMap::default();
    users.insert(user.identity, user.clone());

    Ok(Arc::new(Chatter {
        this_user: user,
        agent,
        printer,
        users: RwLock::new(users),
    }))
}

impl<F> Chatter<F>
where
    F: Future<Output = String> + Send + Sync + 'static,
{
    /// Starts the chat client.
    pub async fn start(&self) -> io::Result<()> {
        self.agent.start().await;
        self.announce_join().await?;

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                eprint!("\x1b[3D");
                self.announce_leave().await?;
                logger::info!("Shutting down...");
            },
            // This also handles Ctrl+C when the printer is waiting for the user to press Enter.
            _ = self.printer.waits_for_enter() => {
                self.announce_leave().await?;
                logger::info!("Printer stopped...");
            },
            _ = async move {
                loop {
                    self.process_delivery(self.agent.output.pop().await).await.expect(
                        "Failed to process delivery.",
                    );
                }
            } => {},
        };

        self.printer.stop().expect("Failed to stop printer.");

        Ok(())
    }

    /// Prints a message to the chat.
    async fn print_message(&self, message: &str, from: &User) -> io::Result<()> {
        self.printer
            .print(&format!(
                "\x1b[1m\x1b[38;5;{colour}m{name}\x1b[39m:\x1b[22m\n\x1b[4G{message}",
                name = from.name,
                colour = from.colour,
                message = message
            ))
            .await
    }

    /// Prints a system message to the chat.
    async fn print_system_message(&self, message: &str) -> io::Result<()> {
        self.printer
            .print(&format!(
                "\u{1F528} \x1b[38;5;{colour}m{message}\x1b[39m",
                message = message,
                colour = config::SYSTEM_COLOUR
            ))
            .await
    }

    /// Announces that the user has joined the chat.
    async fn announce_join(&self) -> io::Result<()> {
        self.agent
            .send(ChatDelivery::Joined {
                identity: self.this_user.identity,
                name: self.this_user.name.clone(),
                colour: self.this_user.colour,
            })
            .await
            .map(|_| ())
    }

    /// Announces that the user has left the chat.
    async fn announce_leave(&self) -> io::Result<()> {
        self.agent
            .send(ChatDelivery::Left {
                identity: self.this_user.identity,
            })
            .await
            .map(|_| ())
    }

    /// Processes an incoming message.
    ///
    /// TODO this can be converted to use Majority-Ack Uniform Reliable Multicast.
    async fn process_delivery(&self, delivery: MulticastDelivery<ChatDelivery>) -> io::Result<()> {
        match delivery.body {
            ChatDelivery::Message { identity, message } => {
                if let Some(user) = self.users.read().await.get(&identity) {
                    self.print_message(&message, user).await
                } else {
                    self.printer.print(
                        format!("\x1b[38;5;246m\x1b[1mSomeone unknown {identity} has sent a message: \x1b[22m{message}\x1b[39m"
                    )).await
                }
            }
            ChatDelivery::Joined {
                identity,
                name,
                colour,
            } => {
                let old = self.users.write().await.insert(
                    identity,
                    User::remote(identity, name.clone(), colour, delivery.sender),
                );
                if old.is_none() {
                    // This is a new user; we should tell them who we are.
                    self.announce_join().await?;
                    self.print_system_message(&format!(
                        "\x1b[38;5;{colour}m{}\x1b[39m has joined the chat!",
                        &name
                    ))
                    .await
                } else {
                    Ok(())
                }
            }
            ChatDelivery::Left { identity } => {
                if let Some(user) = self.users.write().await.remove(&identity) {
                    self.print_system_message(&format!(
                        "\x1b[38;5;{colour}m{name}\x1b[39m has left the chat!",
                        colour = user.colour,
                        name = &user.name
                    ))
                    .await
                } else {
                    self.print_system_message(&format!("\x1b[38;5;246m\x1b[1mSomeone unknown {identity} has left the chat!\x1b[22m\x1b[39m")).await
                }
            }
        }
    }
}
