## Simple Logger to `stderr`

Simple logger that logs to `stderr` with a timestamp and log level; it does not yet
support logging to file streams, or dynamically setting the log level.

It is simply a wrapper around `eprintln!` that prepends the relevant information and
ANSI codes.

### Build

The `src` directory does not contain any meaningful source code; most of the work
is done by `build.rs` which generates the `logger.rs` file at compile time.

The different levels are set by the `src/levels.txt` file in the following format:

```plaintext
0   trace   249 TRACE
10	debug	245	DEBUG
20	info	15	INFO
30	warn	11	WARN
40	error	9	ERROR
```

Each line is `\t` separated, in sequence:
- **Level priority** - the higher the number, the higher the priority.
- **Level name** - the name of the level; this will be the name of the macro.
- **ANSI code** - the ANSI code for the level, between `0` and `255`, to be used with `\x1b[38;5;{code}m`.
- **Level name in uppercase** - the level name in uppercase; this will be shown in the log. The length of this field should be shorter than 8 characters.

### Usage

Using the above levels, you can log messages like so:

```rust
logger::trace!("This is a trace message");
logger::debug!("This is a debug message");
logger::info!("This is an info message");
logger::warn!("This is a warning message");
logger::error!("This is an error message");
```
