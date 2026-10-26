// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The `marked` command: reads JSON-lines requests on standard input and writes one response line per request.

use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::thread;

use marked::cli;

/// The stack of the thread that answers the requests. Nested JSON containers, block quotes, lists and emphasis
/// recurse; the depth limits of the parsers (where JavaScript would throw a `RangeError`) keep the recursion within
/// this size.
const STACK_SIZE: usize = 1 << 30;

fn main() -> ExitCode {
    let worker = thread::Builder::new()
        .name(String::from("marked"))
        .stack_size(STACK_SIZE)
        .spawn(run);
    let outcome = match worker {
        Ok(handle) => handle
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("the request thread stopped"))),
        Err(error) => Err(error),
    };
    match outcome {
        Ok(status) => status,
        Err(error) => {
            // The exit status reports the failure in any case; when standard error cannot be written either, the
            // message has nowhere else to go, so a failure to write it changes nothing.
            if writeln!(io::stderr(), "marked: {error}").is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::FAILURE
        }
    }
}

/// Answers the requests on standard input; the exit status follows the reference command.
fn run() -> io::Result<ExitCode> {
    if std::env::args_os().len() > 1 {
        io::stderr().write_all(cli::USAGE.as_bytes())?;
        return Ok(ExitCode::from(2));
    }
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    let responses = cli::respond(&String::from_utf8_lossy(&bytes));
    let mut stdout = io::stdout().lock();
    stdout.write_all(responses.output.as_bytes())?;
    stdout.flush()?;
    Ok(if responses.failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
