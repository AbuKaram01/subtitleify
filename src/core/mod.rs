// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Pure domain logic: talking to `yt-dlp`/`ffmpeg`, parsing and cleaning
//! subtitle data, and writing output files. Nothing here decides how to
//! respond to a failure or calls `std::process::exit` — every fallible
//! operation reports failure through a `Result` and leaves that decision
//! to the `commands` layer that calls it. A few spots do print an
//! occasional best-effort warning directly (e.g. a malformed json3 file),
//! but never exit on their own.

pub mod downloader;
pub mod merger;
pub mod parser;
pub mod types;
pub mod util;
pub mod writer;
