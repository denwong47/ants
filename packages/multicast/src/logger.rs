#[macro_export]
macro_rules! debug {
    ($base:tt) => {{
        #[cfg(feature="debug")]
        eprintln!("\x1b[1m\x1b[38:5:245mDEBUG\x1b[39m | \x1b[22m\x1b[38:5:245m{}\x1b[39m", $base);
    }};
    ($base:tt, $($arg:tt)*) => {{
        #[cfg(feature="debug")]
        eprintln!(concat!("\x1b[1m\x1b[38:5:245mDEBUG\x1b[39m | \x1b[22m\x1b[38:5:245m", $base, "\x1b[39m"), $($arg)*);
    }};
}

#[macro_export]
macro_rules! info {
    ($base:tt) => {{
        #[cfg(feature="debug")]
        eprintln!("\x1b[1m\x1b[38:5:15mINFO\x1b[39m  | \x1b[22m\x1b[38:5:7m{}\x1b[39m", $base);
    }};
    ($base:tt, $($arg:tt)*) => {{
        #[cfg(feature="debug")]
        eprintln!(concat!("\x1b[1m\x1b[38:5:15mINFO\x1b[39m  | \x1b[22m\x1b[38:5:7m", $base, "\x1b[39m"), $($arg)*);
    }};
}

#[macro_export]
macro_rules! warn {
    ($base:tt) => {{
        eprintln!("\x1b[1m\x1b[38:5:11mWARN\x1b[39m  | \x1b[22m\x1b[38:5:228m{}\x1b[39m", $base);
    }};
    ($base:tt, $($arg:tt)*) => {{
        eprintln!(concat!("\x1b[1m\x1b[38:5:11mWARN\x1b[39m  | \x1b[22m\x1b[38:5:228m", $base, "\x1b[39m"), $($arg)*);
    }};
}

#[macro_export]
macro_rules! error {
    ($base:tt) => {{
        eprintln!("\x1b[1m\x1b[38:5:9mERROR\x1b[39m | \x1b[22m\x1b[38:5:160m{}\x1b[39m", $base);
    }};
    ($base:tt, $($arg:tt)*) => {{
        eprintln!(concat!("\x1b[1m\x1b[38:5:9mERROR\x1b[39m | \x1b[22m\x1b[38:5:160m", $base, "\x1b[39m"), $($arg)*);
    }};
}

pub use crate::{debug, error, info, warn};
