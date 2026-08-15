// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Terminal output helpers shared by every subcommand. Purely presentational
//! — nothing here reads input or makes a decision; it just prints.

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

// ── banner & spinner ──────────────────────────────────────────────────────────

/// Prints the `subtitleify` banner shown once at startup, before any
/// subcommand runs.
pub fn print_banner() {
    println!();
    println!(
        "  {}  {}",
        style("▶").red().bold(),
        style("subtitleify").white().bold()
    );
    println!("  {}", style("YouTube subtitle downloader & cleaner").dim());
    println!("  {}", style("─".repeat(44)).dim());
    println!();
}

/// Prints which browser's cookies are being used for authentication —
/// either the one passed via `--browser` or the one auto-detected.
pub fn show_browser(browser: &str) {
    println!(
        "  {}  browser  {}",
        style("✓").green().bold(),
        style(browser).cyan().bold()
    );
}

/// Builds a spinner pre-styled to match the rest of the CLI's output, with
/// steady ticking already enabled. Callers just need to `finish_and_clear()`
/// it once the work it represents is done.
///
/// In `--verbose` mode, the caller's subprocess is about to print its own
/// raw output — an actively-redrawing spinner sharing the same terminal
/// line would visually tear that apart — so no spinner is drawn at all:
/// `msg` is printed as a plain line instead, and a hidden, no-op
/// `ProgressBar` is returned so every call site can keep calling
/// `.set_message()` / `.finish_and_clear()` on it unconditionally.
pub fn make_spinner(msg: String, verbose: bool) -> ProgressBar {
    if verbose {
        println!("  {}", style(&msg).dim());
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan.bold}  {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(msg);
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Prints the final "N/N files saved" line shared by every download/merge run.
pub fn print_summary(saved: usize, total: usize) {
    println!("  {}", style("─".repeat(44)).dim());
    if saved == total {
        println!(
            "  {}  All {} file{} saved.",
            style("✓").green().bold(),
            style(total).green().bold(),
            if total == 1 { "" } else { "s" }
        );
    } else {
        println!(
            "  {}  {}/{} file{} saved.",
            style("▶").yellow().bold(),
            style(saved).green().bold(),
            total,
            if total == 1 { "" } else { "s" }
        );
    }
    println!();
}
