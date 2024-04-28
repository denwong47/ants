use itertools::*;
use std::env;
use std::fs;
use std::io;
use std::path::Path;

struct LoggingLevel {
    _level: i32,
    name: String,
    display_colour: u8,
    display_title: String,
}

impl LoggingLevel {
    /// Generate the source code for the logging macro.
    fn to_source(&self) -> String {
        format!(
            "
#[macro_export]
macro_rules! {name} {{
    ($base:tt) => {{
        #[cfg(feature=\"debug\")]
        eprintln!(
            concat!(
                \"\\x1b[38:5:245m{{}}\\x1b[39m \u{2502} \",
                \"\\x1b[1m\\x1b[38:5:{ansi}m{title:7} \u{2502} \\x1b[22m{{}}\\x1b[39m\",
            ),
            chrono::Utc::now().to_rfc3339(),
            $base
        );
    }};
    ($base:tt, $($arg:tt)*) => {{
        #[cfg(feature=\"debug\")]
        eprintln!(
            concat!(
                \"\\x1b[38:5:245m{{}}\\x1b[39m \u{2502} \",
                \"\\x1b[1m\\x1b[38:5:{ansi}m{title:7} \u{2502} \\x1b[22m\",
                $base,
                \"\\x1b[39m\"
            ),
            chrono::Utc::now().to_rfc3339(),
            $($arg)*
        );
    }};
}}
        ",
            name = self.name,
            ansi = self.display_colour,
            title = self.display_title,
        )
    }
}

fn line_to_logging_level(line: &str) -> Result<LoggingLevel, io::Error> {
    let mut parts = line.split_whitespace();

    let level = parts
        .next()
        .ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Missing level number: {line}"),
        ))?
        .parse::<i32>()
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Failed to parse level number: {err}"),
            )
        })?;

    let name = parts.next().ok_or(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("Missing name for level {level}: {line}"),
    ))?;

    let display_colour = parts
        .next()
        .ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Missing display colour: {line}"),
        ))?
        .parse::<u8>()
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Failed to parse display colour: {err}"),
            )
        })?;

    let display_title = parts.next().ok_or(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("Missing display title: {line}"),
    ))?;

    Ok(LoggingLevel {
        _level: level,
        name: name.to_string(),
        display_colour,
        display_title: display_title.to_string(),
    })
}

fn main() -> io::Result<()> {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("logger.rs");

    let table_path = Path::new("src").join("levels.txt");

    let table = fs::read_to_string(table_path)?
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| match line_to_logging_level(line) {
            Ok(logging_level) => Some(logging_level),
            Err(err) => {
                eprintln!("\u{1F6D1} {err}");
                None
            }
        })
        .collect::<Vec<_>>();

    fs::write(
        &dest_path,
        format!(
            "/// Generated logging levels.\n\
            /// This file was generated by the build script.\n\
            /// Do not modify this file manually.\n\n\
            {levels}\n",
            levels = table.iter().map(|level| level.to_source()).join("\n\n"),
        ),
    )
    .unwrap_or_else(|_| panic!("Unable to write to file at {dest_path:?}."));

    println!("cargo::rerun-if-changed=src/levels.txt");
    println!("cargo::rerun-if-changed=build.rs");

    Ok(())
}
