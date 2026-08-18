// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! `subtitleify download` and `subtitleify languages`.

use std::fs;
use std::path::PathBuf;

use console::style;

use crate::core::{
    downloader::{
        detect_browser, download_with_retry, fetch_playlist, get_video_title, is_playlist_url,
        is_valid_youtube_url, list_available_subs, po_token_provider_reachable, require_deno,
        require_yt_dlp, resolve_output_dir, temp_id, try_list_available_subs_both,
    },
    parser::process_json3,
    types::{SubFormat, SubType},
    util::{lang_display_name, unique_path},
    writer::{write_srt, write_vtt},
};

use crate::cli::commands::fail;
use crate::cli::ui::{make_spinner, print_summary, show_browser};
use crate::cli::{
    match_available_pairs, parse_format, resolve_type_lang_pairs, validate_download_args,
    DownloadArgs, LanguagesArgs,
};

// ── entry point ───────────────────────────────────────────────────────────────

/// Entry point for `subtitleify download`. Validates dependencies and flags
/// first — `fail`ing fast with a clear message before anything is
/// downloaded — then routes to [`run_single`] or [`run_playlist`] depending
/// on whether `--url` points at a single video or a playlist.
pub fn run(args: DownloadArgs, verbose: bool) {
    require_yt_dlp().unwrap_or_else(|e| fail(e));
    require_deno().unwrap_or_else(|e| fail(e));

    if let Err(missing) = validate_download_args(&args) {
        fail(format!(
            "download requires: {}",
            style(missing.join("  ·  ")).cyan()
        ));
    }

    let browser = resolve_browser(args.browser.as_ref());
    show_browser(&browser);
    warn_if_po_token_provider_unreachable();

    let output_dir: Option<PathBuf> = args.output.as_deref().map(PathBuf::from);

    let url = args.url.as_deref().unwrap().trim().to_string();

    if url.is_empty() {
        fail("No URL provided.");
    }

    if !is_valid_youtube_url(&url) {
        fail(format!("Not a valid YouTube URL: {}", style(&url).dim()));
    }

    let requested = resolve_type_lang_pairs(&args.type_groups).unwrap_or_else(|e| fail(e));
    let cli_fmt = args.format.as_deref().unwrap();

    if is_playlist_url(&url) {
        run_playlist(&url, &browser, &requested, cli_fmt, &output_dir, verbose);
    } else {
        run_single(&url, &browser, &requested, cli_fmt, &output_dir, verbose);
    }
}

// ── list available languages ──────────────────────────────────────────────────

/// Entry point for `subtitleify languages`: shows every subtitle language
/// available for `--url`, for both manual and auto-generated captions, then
/// exits without downloading anything. Meant for users who want to know
/// what's available before picking `download --type` language codes.
///
/// A playlist URL (or a video URL that also carries a `list=` param) is
/// resolved to its first video by default — matching `download`'s own
/// playlist handling — instead of being probed as-is: yt-dlp's `-j` on a
/// playlist prints one JSON object per video, and this command only ever
/// expects one, which produced a "trailing characters" parse error.
/// `--all-videos` opts into checking every video instead and reporting
/// only the languages common to all of them — see
/// [`run_list_languages_all_videos`].
pub fn run_list_languages(args: LanguagesArgs, verbose: bool) {
    require_yt_dlp().unwrap_or_else(|e| fail(e));
    require_deno().unwrap_or_else(|e| fail(e));

    let url = args.url.trim().to_string();

    if url.is_empty() {
        fail("No URL provided.");
    }
    if !is_valid_youtube_url(&url) {
        fail(format!("Not a valid YouTube URL: {}", style(&url).dim()));
    }

    let browser = resolve_browser(args.browser.as_ref());
    show_browser(&browser);
    warn_if_po_token_provider_unreachable();

    if is_playlist_url(&url) && args.all_videos {
        run_list_languages_all_videos(&url, &browser, verbose);
        return;
    }

    let probe_url = if is_playlist_url(&url) {
        resolve_first_video_url(&url, &browser, verbose)
    } else {
        url
    };

    println!();
    let pb = make_spinner("Fetching available languages…".to_string(), verbose);
    let manual = list_available_subs(&probe_url, &SubType::Manual, &browser, verbose);
    let auto = list_available_subs(&probe_url, &SubType::Auto, &browser, verbose);
    pb.finish_and_clear();

    print_language_group("Manual (community)", &manual);
    print_language_group("Auto-generated", &auto);
    println!();
}

/// `subtitleify languages --all-videos` on a playlist: checks every video
/// (one `yt-dlp -j` call each, covering both types at once — see
/// [`try_list_available_subs_both`]) and reports, per type, only the
/// languages common to every video. A video whose availability couldn't
/// be determined (private, deleted, a transient failure, …) is skipped
/// from the intersection entirely rather than treated as having zero
/// languages, so one broken video doesn't silently zero out the whole
/// result — see [`print_common_language_group`] for how that's surfaced.
fn run_list_languages_all_videos(url: &str, browser: &str, verbose: bool) {
    println!();
    let pb = make_spinner("Fetching playlist info…".to_string(), verbose);
    let playlist = match fetch_playlist(url, browser, verbose) {
        Ok(p) => {
            pb.finish_and_clear();
            p
        }
        Err(e) => {
            pb.finish_and_clear();
            fail(format!("{e}"));
        }
    };

    if playlist.videos.is_empty() {
        fail("Playlist is empty.");
    }

    println!(
        "  {}  {}  {}",
        style("▶").dim(),
        style(&playlist.title).bold(),
        style(format!("({} videos)", playlist.videos.len())).dim()
    );
    println!();

    let total = playlist.videos.len();
    let mut manual_common: Option<Vec<String>> = None;
    let mut auto_common: Option<Vec<String>> = None;
    let mut checked = 0usize;

    for (i, video) in playlist.videos.iter().enumerate() {
        let pb = make_spinner(format!("Checking video {}/{total}…", i + 1), verbose);
        let result = try_list_available_subs_both(&video.url, browser, verbose);
        pb.finish_and_clear();

        let (manual, auto) = match result {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        checked += 1;

        manual_common = Some(match manual_common {
            None => manual,
            Some(prev) => prev.into_iter().filter(|l| manual.contains(l)).collect(),
        });
        auto_common = Some(match auto_common {
            None => auto,
            Some(prev) => prev.into_iter().filter(|l| auto.contains(l)).collect(),
        });
    }

    print_common_language_group(
        "Manual (community)",
        &manual_common.unwrap_or_default(),
        checked,
        total,
    );
    print_common_language_group(
        "Auto-generated",
        &auto_common.unwrap_or_default(),
        checked,
        total,
    );
    println!();
}

/// Fetches `url`'s playlist info and returns its first video's URL,
/// `fail`ing with a clear message if the playlist can't be fetched or is
/// empty. Shared by any command that only needs one representative video
/// from a playlist rather than the whole thing.
fn resolve_first_video_url(url: &str, browser: &str, verbose: bool) -> String {
    println!();
    let pb = make_spinner("Fetching playlist info…".to_string(), verbose);
    let playlist = match fetch_playlist(url, browser, verbose) {
        Ok(p) => {
            pb.finish_and_clear();
            p
        }
        Err(e) => {
            pb.finish_and_clear();
            fail(format!("{e}"));
        }
    };

    if playlist.videos.is_empty() {
        fail("Playlist is empty.");
    }

    println!(
        "  {}  {}  {}",
        style("▶").dim(),
        style(&playlist.title).bold(),
        style(format!(
            "(based on first of {} videos)",
            playlist.videos.len()
        ))
        .dim()
    );

    playlist.videos[0].url.clone()
}

/// Prints one `code  Display Name` line, padded to `code_width`, or just
/// the bare code if [`lang_display_name`] doesn't recognize it (avoids an
/// ugly "code   code" repeat).
fn print_language_line(lang: &str, code_width: usize) {
    let name = lang_display_name(lang);
    if name == lang {
        println!("     {}", style(lang).cyan());
    } else {
        println!(
            "     {}  {}",
            style(format!("{lang:<code_width$}")).cyan(),
            style(name).dim()
        );
    }
}

/// Prints one labeled block of `subtitleify languages` output — e.g. all
/// manual or all auto-generated languages — as one `code  Display Name`
/// line per language, or a "none available" line if `languages` is empty.
fn print_language_group(label: &str, languages: &[String]) {
    println!();
    if languages.is_empty() {
        println!(
            "  {}  {}  {}",
            style("✗").red().bold(),
            label,
            style("none available").dim()
        );
        return;
    }
    println!(
        "  {}  {}  {} language{}",
        style("✓").green().bold(),
        label,
        style(languages.len()).cyan().bold(),
        if languages.len() == 1 { "" } else { "s" }
    );
    println!();

    // Pad the *plain* code first, then style it — padding a string that
    // already has ANSI color codes embedded would count those invisible
    // bytes toward the width and throw the alignment off.
    let code_width = languages.iter().map(String::len).max().unwrap_or(0);
    for lang in languages {
        print_language_line(lang, code_width);
    }
}

/// Prints one `--all-videos` result block: the intersection across every
/// checked video, with a header noting how many of the playlist's videos
/// that's based on. `checked` can be less than `total` when some videos
/// were skipped as unreachable (see [`run_list_languages_all_videos`]).
fn print_common_language_group(label: &str, languages: &[String], checked: usize, total: usize) {
    println!();
    if checked == 0 {
        println!(
            "  {}  {}  {}",
            style("✗").red().bold(),
            label,
            style("no videos were reachable").dim()
        );
        return;
    }
    if languages.is_empty() {
        println!(
            "  {}  {}  {}",
            style("✗").red().bold(),
            label,
            style("none common to every video checked").dim()
        );
        return;
    }

    let scope = if checked == total {
        format!("common to all {total} videos")
    } else {
        format!(
            "common to {checked}/{total} — {} unreachable",
            total - checked
        )
    };

    println!(
        "  {}  {}  {} language{}  {}",
        style("✓").green().bold(),
        label,
        style(languages.len()).cyan().bold(),
        if languages.len() == 1 { "" } else { "s" },
        style(format!("({scope})")).dim()
    );
    println!();

    let code_width = languages.iter().map(String::len).max().unwrap_or(0);
    for lang in languages {
        print_language_line(lang, code_width);
    }
}

// ── browser resolution ────────────────────────────────────────────────────────

/// Returns the `--browser` value if one was given, otherwise the
/// auto-detected browser. `fail`s if none was given and auto-detection
/// found nothing installed.
fn resolve_browser(cli_browser: Option<&String>) -> String {
    if let Some(b) = cli_browser {
        return b.clone();
    }
    match detect_browser() {
        Some(b) => b,
        None => fail("No supported browser found. Use --browser to specify one."),
    }
}

// ── PO Token provider check (soft — see core::downloader) ────────────────────

/// Prints a one-line heads-up if no PO Token provider is reachable on its
/// default port. Never fails the run: plenty of videos don't need a PO
/// Token at all, so this is purely a preemptive pointer toward the
/// README's Troubleshooting section, shown upfront instead of only after a
/// confusing "no subtitles" result downstream.
fn warn_if_po_token_provider_unreachable() {
    if !po_token_provider_reachable() {
        eprintln!(
            "  {}  No PO Token provider detected — see README Troubleshooting",
            style("!").yellow().bold()
        );
    }
}

/// Returns `" (manual)"`/`" (auto)"` when `lang` was requested in more
/// than one type within `chosen` — so the resulting files land as
/// `name (manual).vtt`/`name (auto).vtt` instead of the generic, otherwise
/// indistinguishable `name.vtt`/`name (1).vtt` that plain collision
/// numbering would produce. Empty string in the common case where each
/// language only appears once.
fn type_suffix(chosen: &[(String, SubType)], lang: &str, sub_type: &SubType) -> String {
    if chosen.iter().filter(|(l, _)| l == lang).count() > 1 {
        format!(" ({})", sub_type.label())
    } else {
        String::new()
    }
}

// ── language selection ────────────────────────────────────────────────────────

/// Fetches the subtitle languages available for `probe_url` — querying
/// manual, auto, or both, depending on which types `requested` actually
/// needs — and resolves `requested` down to the pairs that exist.
///
/// `note`, if given, is appended after the "available" line (used by the
/// playlist path to clarify the count is based on the first video).
/// Returns `None` when nothing at all is available for `probe_url` in any
/// needed type; `fail`s (rather than returning `None`) if languages exist
/// but none of them match what was requested, since that's a more precise
/// signal — the user asked for a specific language that isn't there.
fn select_languages(
    probe_url: &str,
    browser: &str,
    requested: &[(String, SubType)],
    note: Option<&str>,
    verbose: bool,
) -> Option<Vec<(String, SubType)>> {
    let need_manual = requested.iter().any(|(_, t)| *t == SubType::Manual);
    let need_auto = requested.iter().any(|(_, t)| *t == SubType::Auto);

    let pb = make_spinner("Fetching available subtitles…".to_string(), verbose);
    let manual = if need_manual {
        list_available_subs(probe_url, &SubType::Manual, browser, verbose)
    } else {
        Vec::new()
    };
    let auto = if need_auto {
        list_available_subs(probe_url, &SubType::Auto, browser, verbose)
    } else {
        Vec::new()
    };
    pb.finish_and_clear();

    if manual.is_empty() && auto.is_empty() {
        return None;
    }

    print!("  {}  ", style("✓").green().bold());
    if need_manual && need_auto {
        print!(
            "{} manual · {} auto available",
            style(manual.len()).cyan().bold(),
            style(auto.len()).cyan().bold()
        );
    } else if need_manual {
        print!(
            "{} language{} available",
            style(manual.len()).cyan().bold(),
            if manual.len() == 1 { "" } else { "s" }
        );
    } else {
        print!(
            "{} language{} available",
            style(auto.len()).cyan().bold(),
            if auto.len() == 1 { "" } else { "s" }
        );
    }
    if let Some(n) = note {
        print!("  {}", style(n).dim());
    }
    println!();
    println!();

    let matched = match_available_pairs(requested, &manual, &auto).unwrap_or_else(|e| fail(e));
    Some(matched)
}

// ── single video download ─────────────────────────────────────────────────────

/// Downloads every requested language for one video, writing each straight
/// to `output_dir` (or the default Downloads folder). Prints a per-language
/// progress line as it goes and a final saved/total summary.
fn run_single(
    url: &str,
    browser: &str,
    requested: &[(String, SubType)],
    cli_fmt: &str,
    output_dir: &Option<PathBuf>,
    verbose: bool,
) {
    println!();
    let chosen = match select_languages(url, browser, requested, None, verbose) {
        Some(c) => c,
        None => {
            eprintln!(
                "  {}  No subtitles found for this video.",
                style("✗").red().bold()
            );
            return;
        }
    };

    let format = parse_format(cli_fmt);

    println!();
    let pb = make_spinner("Fetching video title…".to_string(), verbose);
    let title = get_video_title(url, browser, verbose);
    pb.finish_and_clear();
    println!("  {}  {}", style("▶").dim(), style(&title).bold());

    println!();
    println!("  {}", style("─".repeat(44)).dim());
    println!();

    let downloads = resolve_output_dir(output_dir);
    let total = chosen.len();
    let mut saved = 0usize;

    for (n, (lang, sub_type)) in chosen.iter().enumerate() {
        let temp_prefix = format!("temp_subs_{}_{}", lang, temp_id());
        let pb = make_spinner(
            format!(
                "[{}/{}]  {}  Downloading…",
                n + 1,
                total,
                style(lang).cyan().bold()
            ),
            verbose,
        );
        let mut temp_files: Vec<String> = Vec::new();

        let result: Result<String, Box<dyn std::error::Error>> = (|| {
            let json3_path =
                download_with_retry(url, lang, sub_type, &temp_prefix, browser, 3, verbose)?;
            temp_files.push(json3_path.clone());
            pb.set_message(format!(
                "[{}/{}]  {}  Processing…",
                n + 1,
                total,
                style(lang).cyan().bold()
            ));
            let cues = process_json3(&json3_path)?;
            let suffix = type_suffix(&chosen, lang, sub_type);
            let filename = match format {
                SubFormat::Vtt => format!("{title} - {lang}{suffix}.vtt"),
                SubFormat::Srt => format!("{title} - {lang}{suffix}.srt"),
            };
            let out_path = unique_path(&downloads.join(&filename));
            fs::write(
                &out_path,
                match format {
                    SubFormat::Vtt => write_vtt(&cues, lang),
                    SubFormat::Srt => write_srt(&cues),
                }
                .as_bytes(),
            )?;
            Ok(out_path.to_string_lossy().into_owned())
        })();

        for p in &temp_files {
            let _ = fs::remove_file(p);
        }
        pb.finish_and_clear();
        match &result {
            Ok(p) => {
                saved += 1;
                println!(
                    "  {}  {}  {}",
                    style("✓").green().bold(),
                    style(lang).cyan().bold(),
                    style(p).dim()
                );
            }
            Err(e) => eprintln!(
                "  {}  {}  {}",
                style("✗").red().bold(),
                style(lang).cyan().bold(),
                style(e.to_string()).red()
            ),
        }
    }

    println!();
    print_summary(saved, total);
}

// ── playlist download ─────────────────────────────────────────────────────────

/// Downloads every requested language for every video in a playlist, into a
/// dedicated `"{playlist title} subs"` subfolder. Language availability is
/// probed against the *first* video only — a video-by-video probe would be
/// slow and, in practice, sibling videos in a playlist almost always share
/// the same caption languages.
fn run_playlist(
    url: &str,
    browser: &str,
    requested: &[(String, SubType)],
    cli_fmt: &str,
    output_dir: &Option<PathBuf>,
    verbose: bool,
) {
    println!();
    let pb = make_spinner("Fetching playlist info…".to_string(), verbose);
    let playlist = match fetch_playlist(url, browser, verbose) {
        Ok(p) => {
            pb.finish_and_clear();
            p
        }
        Err(e) => {
            pb.finish_and_clear();
            eprintln!("  {}  {}", style("✗").red().bold(), e);
            return;
        }
    };

    println!(
        "  {}  {}  {}",
        style("▶").dim(),
        style(&playlist.title).bold(),
        style(format!("({} videos)", playlist.videos.len())).dim()
    );

    if playlist.videos.is_empty() {
        eprintln!("  {}  Playlist is empty.", style("✗").red().bold());
        return;
    }

    println!();
    let chosen = match select_languages(
        &playlist.videos[0].url,
        browser,
        requested,
        Some("(based on first video)"),
        verbose,
    ) {
        Some(c) => c,
        None => {
            eprintln!(
                "  {}  No subtitles found on the first video.",
                style("✗").red().bold()
            );
            return;
        }
    };

    let format = parse_format(cli_fmt);

    let folder_path = resolve_output_dir(output_dir).join(format!("{} subs", playlist.title));
    if let Err(e) = fs::create_dir_all(&folder_path) {
        eprintln!("  {}  {}", style("✗").red().bold(), e);
        return;
    }

    println!();
    println!(
        "  {}  {}",
        style("▶").dim(),
        style(folder_path.display()).bold()
    );
    println!();
    println!("  {}", style("─".repeat(44)).dim());
    println!();

    let total = playlist.videos.len() * chosen.len();
    let mut saved = 0usize;
    let mut n = 0usize;

    for video in &playlist.videos {
        for (lang, sub_type) in &chosen {
            n += 1;
            let temp_prefix = format!("temp_subs_{}_{}", lang, temp_id());
            let pb = make_spinner(
                format!(
                    "[{n}/{total}]  {}  {}",
                    style(lang).cyan().bold(),
                    style(&video.title).dim()
                ),
                verbose,
            );
            let mut temp_files: Vec<String> = Vec::new();

            let result: Result<String, Box<dyn std::error::Error>> = (|| {
                let json3_path = download_with_retry(
                    &video.url,
                    lang,
                    sub_type,
                    &temp_prefix,
                    browser,
                    3,
                    verbose,
                )?;
                temp_files.push(json3_path.clone());
                pb.set_message(format!(
                    "[{n}/{total}]  {}  Processing…",
                    style(lang).cyan().bold()
                ));
                let cues = process_json3(&json3_path)?;
                let suffix = type_suffix(&chosen, lang, sub_type);
                let filename = match format {
                    SubFormat::Vtt => format!("{} - {}{suffix}.vtt", video.title, lang),
                    SubFormat::Srt => format!("{} - {}{suffix}.srt", video.title, lang),
                };
                let out_path = unique_path(&folder_path.join(&filename));
                fs::write(
                    &out_path,
                    match format {
                        SubFormat::Vtt => write_vtt(&cues, lang),
                        SubFormat::Srt => write_srt(&cues),
                    }
                    .as_bytes(),
                )?;
                Ok(out_path.file_name().unwrap().to_string_lossy().into_owned())
            })();

            for p in &temp_files {
                let _ = fs::remove_file(p);
            }
            pb.finish_and_clear();
            match &result {
                Ok(f) => {
                    saved += 1;
                    println!(
                        "  {}  {}  {}",
                        style("✓").green().bold(),
                        style(lang).cyan().bold(),
                        style(f).dim()
                    );
                }
                Err(e) => eprintln!(
                    "  {}  {}  {}",
                    style("✗").red().bold(),
                    style(lang).cyan().bold(),
                    style(e.to_string()).red()
                ),
            }
        }
    }

    println!();
    print_summary(saved, total);
}
