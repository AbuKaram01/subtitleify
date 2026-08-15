// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! subtitleify — a flag-driven CLI for downloading, cleaning, and merging
//! YouTube subtitles.
//!
//! This is a binary-only crate with two top-level modules: `core` holds
//! the pure domain logic (no printing, no `process::exit`), and `cli`
//! holds everything about the command line — the flag definitions
//! themselves, plus (in `cli::commands`) the orchestration layer that
//! wires those flags to `core` and is the only place allowed to print or
//! exit.

mod cli;
mod core;

use clap::Parser;

use cli::commands;
use cli::ui::print_banner;
use cli::{Cli, Commands};

/// Parses the CLI flags and dispatches to the matching subcommand. Every
/// subcommand is required — there is no interactive fallback, so `clap`
/// itself handles printing usage/help when the invocation is incomplete.
fn main() {
    let cli = Cli::parse();
    print_banner();

    match cli.command {
        Commands::Download(args) => commands::download::run(args, cli.verbose),
        Commands::Languages(args) => commands::download::run_list_languages(args, cli.verbose),
        Commands::Merge { mode } => commands::merge::run(mode, cli.verbose),
    }
}
