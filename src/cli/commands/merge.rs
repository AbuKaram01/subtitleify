// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! `subtitleify merge` (both `folder` and `single` modes).

use std::fs;
use std::path::{Path, PathBuf};

use console::style;

use crate::core::{
    downloader::require_ffmpeg,
    merger::{match_videos_to_subs, merge_single, merge_video},
};

use crate::cli::commands::fail;
use crate::cli::ui::{make_spinner, print_summary};
use crate::cli::MergeCommand;

// ── entry point ───────────────────────────────────────────────────────────────

/// Entry point for `subtitleify merge`. Checks `ffmpeg` is available, then
/// routes to [`run_merge_folder`] or [`run_merge_single`] depending on
/// which merge subcommand was given.
pub fn run(mode: MergeCommand, verbose: bool) {
    require_ffmpeg().unwrap_or_else(|e| fail(e));

    match mode {
        MergeCommand::Folder(args) => {
            let videos_dir = PathBuf::from(args.videos_dir);
            let subs_dir = PathBuf::from(args.subs_dir);
            let output_dir = args.output.map(PathBuf::from);
            run_merge_folder(&videos_dir, &subs_dir, &output_dir, verbose);
        }
        MergeCommand::Single(args) => {
            let video = PathBuf::from(args.video);
            let sub_paths: Vec<PathBuf> = args.sub.into_iter().map(PathBuf::from).collect();
            let output_dir = args.output.map(PathBuf::from);
            run_merge_single(&video, &sub_paths, &output_dir, verbose);
        }
    }
}

// ── merge folder ──────────────────────────────────────────────────────────────

/// Matches every video in `videos_dir` with its subtitles in `subs_dir`
/// (see [`match_videos_to_subs`]) and merges each pair with `ffmpeg`.
/// Defaults `output_dir` to `"{videos_dir name} merged"` next to
/// `videos_dir` when not given. Prints a per-video progress line, flags
/// any position-matched (stage 3) pairs as worth double-checking, and
/// finishes with a saved/total summary.
fn run_merge_folder(
    videos_dir: &Path,
    subs_dir: &Path,
    output_dir: &Option<PathBuf>,
    verbose: bool,
) {
    if !videos_dir.is_dir() {
        eprintln!("  {}  Videos folder not found.", style("✗").red().bold());
        return;
    }
    if !subs_dir.is_dir() {
        eprintln!("  {}  Subtitles folder not found.", style("✗").red().bold());
        return;
    }

    println!();
    let pb = make_spinner("Matching videos with subtitles…".to_string(), verbose);
    let (jobs, unmatched) = match_videos_to_subs(videos_dir, subs_dir);
    pb.finish_and_clear();

    if jobs.is_empty() {
        eprintln!("  {}  No matches found.", style("✗").red().bold());
        return;
    }

    println!(
        "  {}  {} video{} matched",
        style("✓").green().bold(),
        style(jobs.len()).cyan().bold(),
        if jobs.len() == 1 { "" } else { "s" }
    );

    for p in &unmatched {
        eprintln!(
            "  {}  no subtitles for: {}",
            style("!").yellow().bold(),
            style(p.file_name().unwrap().to_string_lossy()).dim()
        );
    }

    let stage3 = jobs.iter().filter(|j| j.match_stage == 3).count();
    if stage3 > 0 {
        eprintln!(
            "  {}  {} video{} matched by position — verify manually",
            style("!").yellow().bold(),
            stage3,
            if stage3 == 1 { "" } else { "s" }
        );
    }

    let output_dir = match output_dir {
        Some(p) => p.clone(),
        None => videos_dir.parent().unwrap_or(Path::new(".")).join(format!(
            "{} merged",
            videos_dir.file_name().unwrap().to_string_lossy()
        )),
    };

    if let Err(e) = fs::create_dir_all(&output_dir) {
        eprintln!("  {}  {}", style("✗").red().bold(), e);
        return;
    }

    println!();
    println!(
        "  {}  {}",
        style("▶").dim(),
        style(output_dir.display()).bold()
    );
    println!();
    println!("  {}", style("─".repeat(44)).dim());
    println!();

    let total = jobs.len();
    let mut saved = 0usize;

    for (n, job) in jobs.iter().enumerate() {
        let stage_label = match job.match_stage {
            1 => style("id   ").green(),
            2 => style("title").cyan(),
            _ => style("pos  ").yellow(),
        };
        let pb = make_spinner(
            format!(
                "[{}/{}]  {}  {}",
                n + 1,
                total,
                stage_label,
                style(&job.output_name).dim()
            ),
            verbose,
        );
        let result = merge_video(job, &output_dir, verbose);
        pb.finish_and_clear();

        match result {
            Ok(path) => {
                saved += 1;
                println!(
                    "  {}  [{}]  {}",
                    style("✓").green().bold(),
                    stage_label,
                    style(path.file_name().unwrap().to_string_lossy()).dim()
                );
            }
            Err(e) => eprintln!(
                "  {}  {}  {}",
                style("✗").red().bold(),
                style(&job.output_name).dim(),
                style(e.to_string()).red()
            ),
        }
    }

    println!();
    print_summary(saved, total);
}

// ── merge single ──────────────────────────────────────────────────────────────

/// Merges one video with one or more subtitle files, checking every input
/// path exists first. Defaults `output_dir` to alongside the source video
/// when not given (see [`merge_single`] for how a name clash with the
/// source itself is avoided).
fn run_merge_single(
    video_path: &Path,
    sub_paths: &[PathBuf],
    output_dir: &Option<PathBuf>,
    verbose: bool,
) {
    if !video_path.is_file() {
        eprintln!(
            "  {}  Video file not found: {}",
            style("✗").red().bold(),
            video_path.display()
        );
        return;
    }
    for sub in sub_paths {
        if !sub.is_file() {
            eprintln!(
                "  {}  Subtitle file not found: {}",
                style("✗").red().bold(),
                sub.display()
            );
            return;
        }
    }

    if let Some(dir) = output_dir {
        if let Err(e) = fs::create_dir_all(dir) {
            eprintln!("  {}  {}", style("✗").red().bold(), e);
            return;
        }
    }

    println!();
    println!(
        "  {}  {}",
        style("▶").dim(),
        style(video_path.file_name().unwrap().to_string_lossy()).bold()
    );
    println!("  {}", style("─".repeat(44)).dim());
    println!();

    let pb = make_spinner("Merging…".to_string(), verbose);
    let result = merge_single(video_path, sub_paths, output_dir.as_deref(), verbose);
    pb.finish_and_clear();

    match result {
        Ok(output) => {
            println!(
                "  {}  {}",
                style("✓").green().bold(),
                style(output.display()).dim()
            );
            println!();
            print_summary(1, 1);
        }
        Err(e) => eprintln!(
            "  {}  {}",
            style("✗").red().bold(),
            style(e.to_string()).red()
        ),
    }
}
