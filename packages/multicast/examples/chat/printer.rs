//! A locked printer that prints to stdout.
//!

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use futures::StreamExt;
use std::{
    future::Future,
    io::{self, Write},
    sync::{Arc, OnceLock},
};

use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
};

use multicast::logger;

pub struct Printer<F>
where
    F: Future<Output = String> + Send + Sync + 'static,
{
    prompt: String,
    upon_input: Box<dyn Send + Sync + Fn(&Self, String) -> F>,
    _stdout_lock: Mutex<()>,
    _terminate_flag: Arc<Notify>,
    _input_listener: OnceLock<JoinHandle<()>>,
}

impl<F> Drop for Printer<F>
where
    F: Future<Output = String> + Send + Sync + 'static,
{
    /// Aborts the input listener if it exists.
    fn drop(&mut self) {
        self._terminate_flag.notify_one();
        if let Some(handle) = self._input_listener.take() {
            handle.abort();

            disable_raw_mode().expect("Failed to disable raw mode.");
        }
    }
}

impl<F> Printer<F>
where
    F: Future<Output = String> + Send + Sync + 'static,
{
    /// Creates a new printer that prints to stdout.
    pub fn new(
        prompt: impl ToString,
        upon_input: impl Fn(&Self, String) -> F + Send + Sync + 'static,
    ) -> Arc<Self> {
        Self {
            prompt: prompt.to_string(),
            upon_input: Box::new(upon_input),
            _stdout_lock: Mutex::new(()),
            _terminate_flag: Arc::new(Notify::new()),
            _input_listener: OnceLock::new(),
        }
        .into()
    }

    /// Prints the given message.
    pub async fn print(&self, message: impl ToString) -> io::Result<()> {
        let _lock = self._stdout_lock.lock().await;

        println!("\x1b[1G{}", message.to_string());
        // Reset the cursor position.
        print!("\x1b[1G");

        Ok(())
    }

    /// Prints the given prompt, then waits for input.
    pub async fn input(&self) -> io::Result<String> {
        let _lock = self._stdout_lock.lock().await;

        print!("\x1b[1G{}", self.prompt);
        disable_raw_mode()?;
        io::stdout().flush()?;

        let stdin = io::stdin();
        let mut buffer = String::new();

        stdin.read_line(&mut buffer)?;
        // Reset the cursor position.
        print!("\x1b[1G");

        enable_raw_mode()?;
        Ok(buffer.trim().to_owned())
    }

    /// Starts the input listener.
    pub async fn waits_for_enter(self: &Arc<Self>) -> io::Result<()> {
        let mut reader = EventStream::new();

        enable_raw_mode()?;
        loop {
            if let Some(event) = tokio::select!(
                _ = self._terminate_flag.notified() => None,
                result = reader.next() => result,
            ) {
                match event {
                    Ok(Event::Key(event)) if event.code == KeyCode::Enter => {
                        // The user has pressed the enter key, so we should read the input.
                        // We'll then call the `upon_input` function with the input.
                        //
                        // All this is just an `and_then`, but since we can't use `await` in a closure,
                        // we have to use a `match` statement instead.
                        let result = match self.input().await {
                            Ok(input) => Ok((self.upon_input)(self, input).await),
                            Err(err) => Err(err),
                        };

                        if let Ok(message) = result {
                            self.print(message).await?;
                        } else if let Err(err) = result {
                            logger::error!("An error occurred: {}", err);
                        }
                    }
                    Ok(Event::Key(event))
                        if event.code == KeyCode::Char('c')
                            && event.modifiers == KeyModifiers::CONTROL =>
                    {
                        // The user has pressed the escape key, so we should stop the input listener.
                        eprint!("\x1b[3D");
                        logger::info!("Aborting...");
                        break;
                    }
                    _ => {}
                }
            } else {
                break;
            }
        }

        disable_raw_mode().expect("Failed to disable raw mode.");
        logger::debug!("Input listener stopped.");

        Ok(())
    }

    /// Stops the input listener.
    pub fn stop(&self) -> io::Result<()> {
        self._terminate_flag.notify_one();
        if let Some(handle) = self._input_listener.get() {
            logger::debug!("Stopping the input listener...");
            handle.abort();

            disable_raw_mode()?;
        }

        Ok(())
    }
}
