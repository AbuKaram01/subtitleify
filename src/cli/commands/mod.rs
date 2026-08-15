// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Orchestration layer: wires the flags `cli::mod` parses to `core`
//! (domain logic) for each top-level command. This is the only module
//! allowed to call `std::process::exit` — `core` reports problems via
//! `Result`, `cli::mod` collects input, and `commands` decides what a
//! failure means for the run.

pub mod download;
pub mod merge;

use console::style;

/// Prints a formatted error line and exits with status 1. The single place
/// that turns a `Result::Err` from validation or a dependency check into
/// the CLI's standard error format.
pub fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("\n  {}  {}\n", style("✗").red().bold(), msg);
    std::process::exit(1);
}
