// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Flag definitions (`clap`) and pure flag-parsing/validation helpers.
//!
//! This file only defines *what* can be typed on the command line and
//! checks that it's well-formed — it doesn't touch the network, the
//! filesystem, or `core`'s domain logic. `///` doc comments on struct
//! fields and enum variants here double as the `--help` text `clap`
//! generates, so keep them user-facing.
//!
//! `commands` (below) is a different story: it's the orchestration layer
//! that wires this file's parsed flags to `core`, and it's the only place
//! allowed to print to the terminal or call `std::process::exit`.

pub mod commands;
pub mod ui;

use clap::{Args, Parser, Subcommand};
use console::style;

use crate::core::types::{SubFormat, SubType};

// ── CLI definition ────────────────────────────────────────────────────────────

/// Top-level CLI entry point. `command` is required — there's no bare
/// `subtitleify` invocation — so `clap` prints usage on its own if it's
/// omitted.
#[derive(Parser)]
#[command(
    name = "subtitleify",
    version,
    about = "Download & clean YouTube subtitles",
    long_about = "\
Every subcommand is driven entirely by flags — no interactive prompts.\n\n\
  Download        : subtitleify download --url <URL> --type <TYPE> <LANGS> --format <FORMAT>\n\
  List languages  : subtitleify languages --url <URL>\n\
  Merge folder    : subtitleify merge folder --videos-dir <PATH> --subs-dir <PATH>\n\
  Merge single    : subtitleify merge single --video <PATH> --sub <PATH> [--sub <PATH> ...]"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Show yt-dlp/ffmpeg's raw output instead of hiding it
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

/// The three things subtitleify can do.
#[derive(Subcommand)]
pub enum Commands {
    /// Download subtitles from a video or playlist
    Download(DownloadArgs),
    /// List available subtitle languages for a URL
    Languages(LanguagesArgs),
    /// Merge subtitles into videos using ffmpeg
    Merge {
        #[command(subcommand)]
        mode: MergeCommand,
    },
}

/// Flags for `subtitleify download`. `url`, `type_groups`, and `format`
/// are all required — see [`validate_download_args`].
#[derive(Args, Default)]
pub struct DownloadArgs {
    /// YouTube video or playlist URL
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// TYPE (manual/auto) + LANGS; repeat --type to mix types
    #[arg(short = 't', long = "type", value_names = ["TYPE", "LANGS"], num_args = 2, action = clap::ArgAction::Append)]
    pub type_groups: Vec<String>,

    /// Output format: vtt or srt
    #[arg(short, long, value_name = "FORMAT", value_parser = ["vtt", "srt"])]
    pub format: Option<String>,

    /// Output folder (default: Downloads)
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<String>,

    /// Browser for cookies (auto-detected if omitted)
    #[arg(short, long, value_name = "BROWSER",
          value_parser = ["firefox", "chrome", "brave", "edge", "chromium", "opera", "vivaldi"])]
    pub browser: Option<String>,
}

/// Flags for `subtitleify languages`.
#[derive(Args)]
pub struct LanguagesArgs {
    /// YouTube video or playlist URL
    #[arg(long, value_name = "URL")]
    pub url: String,

    /// Check every video in a playlist (default: first only)
    #[arg(long)]
    pub all_videos: bool,

    /// Browser for cookies (auto-detected if omitted)
    #[arg(short, long, value_name = "BROWSER",
          value_parser = ["firefox", "chrome", "brave", "edge", "chromium", "opera", "vivaldi"])]
    pub browser: Option<String>,
}

/// The two ways `subtitleify merge` can be invoked.
#[derive(Subcommand)]
pub enum MergeCommand {
    /// Merge a folder of videos with a folder of subtitles
    Folder(FolderMergeArgs),
    /// Merge one video with one or more subtitle files
    Single(SingleMergeArgs),
}

/// Flags for `subtitleify merge folder`.
#[derive(Args)]
pub struct FolderMergeArgs {
    /// Videos folder path
    #[arg(long, value_name = "PATH")]
    pub videos_dir: String,

    /// Subtitles folder path
    #[arg(long, value_name = "PATH")]
    pub subs_dir: String,

    /// Output folder (default: alongside videos)
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<String>,
}

/// Flags for `subtitleify merge single`.
#[derive(Args)]
pub struct SingleMergeArgs {
    /// Video file path
    #[arg(long, value_name = "PATH")]
    pub video: String,

    /// Subtitle file, repeatable
    #[arg(long = "sub", value_name = "PATH", required = true)]
    pub sub: Vec<String>,

    /// Output folder (default: alongside video)
    #[arg(short = 'o', long, value_name = "PATH")]
    pub output: Option<String>,
}

// ── download flag validation ──────────────────────────────────────────────────
//
// clap's derive API can't express "all of these flags are required together"
// directly while still sharing one struct with `--output`/`--browser`, so
// this stays hand-rolled — but it just reports the outcome. It never prints
// or exits; the `commands` layer decides what to do with the result.

/// Checks that every flag `download` needs was supplied. Returns the list of
/// missing flag names when one or more is absent.
pub fn validate_download_args(args: &DownloadArgs) -> Result<(), Vec<&'static str>> {
    let missing: Vec<&'static str> = [
        args.url.is_none().then_some("--url"),
        args.type_groups.is_empty().then_some("--type"),
        args.format.is_none().then_some("--format"),
    ]
    .into_iter()
    .flatten()
    .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

// ── flag parsers (pure — no printing, no exiting) ─────────────────────────────

/// Converts the validated `--type` value (`"manual"` or `"auto"`) into a
/// [`SubType`]. Anything other than `"manual"` falls back to `Auto`, but in
/// practice `clap`'s `value_parser` on `--type` already rejects every other
/// value before this is ever called.
pub fn parse_sub_type(s: &str) -> SubType {
    match s {
        "manual" => SubType::Manual,
        _ => SubType::Auto,
    }
}

/// Converts the validated `--format` value (`"vtt"` or `"srt"`) into a
/// [`SubFormat`]. Anything other than `"vtt"` falls back to `Srt`, but as
/// with [`parse_sub_type`], `clap` already restricts `--format` to the two
/// valid values before this runs.
pub fn parse_format(s: &str) -> SubFormat {
    match s {
        "vtt" => SubFormat::Vtt,
        _ => SubFormat::Srt,
    }
}

/// Resolves `--type`'s flat `[TYPE, LANGS, TYPE, LANGS, ...]` value list
/// (one `TYPE`/`LANGS` pair per `--type` occurrence — `clap`'s
/// `num_args = 2` guarantees the list's length is always even) into an
/// explicit, ordered list of `(language, subtitle-type)` pairs to
/// attempt — before anything is checked against what's actually available
/// (see [`crate::cli::commands::download`]'s `select_languages`, which does
/// that next).
///
/// One occurrence covers the common case — `--type auto en,ar` requests
/// both as auto. Repeating the flag mixes types in one command:
/// `--type manual en,fr --type auto ar,de` requests `en`/`fr` as manual
/// and `ar`/`de` as auto. The same language can appear under more than
/// one type across occurrences; each is downloaded separately.
pub fn resolve_type_lang_pairs(type_groups: &[String]) -> Result<Vec<(String, SubType)>, String> {
    let mut pairs = Vec::new();

    for chunk in type_groups.chunks(2) {
        let type_token = &chunk[0];
        let langs = &chunk[1];

        if type_token != "manual" && type_token != "auto" {
            return Err(format!(
                "invalid --type value '{type_token}' — must be 'manual' or 'auto'"
            ));
        }
        let sub_type = parse_sub_type(type_token);

        for lang in langs.split(',').map(str::trim) {
            if !lang.is_empty() {
                pairs.push((lang.to_string(), sub_type.clone()));
            }
        }
    }

    if pairs.is_empty() {
        return Err("--type requires a language after it, e.g. --type auto en,ar".to_string());
    }

    Ok(pairs)
}

/// Matches each requested `(language, type)` pair against what's actually
/// available for that specific type (`manual_available`/`auto_available`
/// come from a separate `list_available_subs` call per type actually
/// requested — see `select_languages`). Warns about — and skips — any
/// pair that isn't available; errors only when none of them are.
///
/// The confirmation line and warnings only mention the type explicitly
/// when `requested` actually mixes manual and auto — a single-type
/// request (the common case) prints exactly as it always has.
pub fn match_available_pairs(
    requested: &[(String, SubType)],
    manual_available: &[String],
    auto_available: &[String],
) -> Result<Vec<(String, SubType)>, String> {
    let mixed = requested.iter().any(|(_, t)| *t == SubType::Manual)
        && requested.iter().any(|(_, t)| *t == SubType::Auto);

    let mut matched = Vec::new();

    for (lang, sub_type) in requested {
        let available = match sub_type {
            SubType::Manual => manual_available,
            SubType::Auto => auto_available,
        };
        if available.iter().any(|l| l == lang) {
            matched.push((lang.clone(), sub_type.clone()));
        } else if mixed {
            eprintln!(
                "  {}  language '{}' ({}) not available — skipping.",
                style("!").yellow().bold(),
                style(lang).cyan(),
                sub_type.label()
            );
        } else {
            eprintln!(
                "  {}  language '{}' not available — skipping.",
                style("!").yellow().bold(),
                style(lang).cyan()
            );
        }
    }

    if matched.is_empty() {
        return Err("None of the requested languages are available.".to_string());
    }

    let chosen: Vec<String> = if mixed {
        matched
            .iter()
            .map(|(l, t)| format!("{l} ({})", t.label()))
            .collect()
    } else {
        matched.iter().map(|(l, _)| l.clone()).collect()
    };
    println!(
        "  {}  {}",
        style("✓").green().bold(),
        style(chosen.join("  ·  ")).cyan()
    );

    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(url: bool, type_group: bool, format: bool) -> DownloadArgs {
        DownloadArgs {
            url: url.then(|| "https://youtube.com/watch?v=x".to_string()),
            type_groups: if type_group {
                vec!["auto".to_string(), "en".to_string()]
            } else {
                Vec::new()
            },
            format: format.then(|| "srt".to_string()),
            output: None,
            browser: None,
        }
    }

    #[test]
    fn all_flags_present_is_ok() {
        assert_eq!(validate_download_args(&args(true, true, true)), Ok(()));
    }

    #[test]
    fn no_flags_reports_all_as_missing() {
        let missing = validate_download_args(&args(false, false, false)).unwrap_err();
        assert_eq!(missing, vec!["--url", "--type", "--format"]);
    }

    #[test]
    fn partial_flags_report_exactly_whats_missing() {
        let missing = validate_download_args(&args(true, false, false)).unwrap_err();
        assert_eq!(missing, vec!["--type", "--format"]);
    }

    #[test]
    fn single_occurrence_covers_multiple_languages() {
        let pairs = resolve_type_lang_pairs(&[s("auto"), s("en,ar")]).unwrap();
        assert_eq!(
            pairs,
            vec![
                ("en".to_string(), SubType::Auto),
                ("ar".to_string(), SubType::Auto),
            ]
        );
    }

    #[test]
    fn repeated_flag_mixes_types() {
        let pairs =
            resolve_type_lang_pairs(&[s("manual"), s("en,fr"), s("auto"), s("ar,de")]).unwrap();
        assert_eq!(
            pairs,
            vec![
                ("en".to_string(), SubType::Manual),
                ("fr".to_string(), SubType::Manual),
                ("ar".to_string(), SubType::Auto),
                ("de".to_string(), SubType::Auto),
            ]
        );
    }

    #[test]
    fn same_language_can_appear_under_both_types() {
        let pairs = resolve_type_lang_pairs(&[s("manual"), s("ar"), s("auto"), s("ar")]).unwrap();
        assert_eq!(
            pairs,
            vec![
                ("ar".to_string(), SubType::Manual),
                ("ar".to_string(), SubType::Auto),
            ]
        );
    }

    #[test]
    fn empty_type_groups_is_rejected() {
        assert!(resolve_type_lang_pairs(&[]).is_err());
    }

    #[test]
    fn invalid_type_token_is_rejected() {
        let err = resolve_type_lang_pairs(&[s("mannual"), s("en,ar")]).unwrap_err();
        assert!(err.contains("mannual"));
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn match_available_pairs_skips_unavailable_and_keeps_the_rest() {
        let requested = vec![
            ("en".to_string(), SubType::Manual),
            ("ar".to_string(), SubType::Auto),
        ];
        let manual = vec!["fr".to_string()]; // "en" not available as manual
        let auto = vec!["ar".to_string(), "fr".to_string()];

        let matched = match_available_pairs(&requested, &manual, &auto).unwrap();
        assert_eq!(matched, vec![("ar".to_string(), SubType::Auto)]);
    }

    #[test]
    fn match_available_pairs_errors_when_nothing_matches() {
        let requested = vec![("en".to_string(), SubType::Auto)];
        let manual: Vec<String> = vec![];
        let auto: Vec<String> = vec!["ar".to_string()];

        assert!(match_available_pairs(&requested, &manual, &auto).is_err());
    }
}
