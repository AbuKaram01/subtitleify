// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 AbuKaram01

//! Everything that shells out to `yt-dlp`: dependency checks, browser
//! detection, playlist/language listing, and the actual subtitle download.

use glob::glob;
use serde_json::Value;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::types::{Playlist, PlaylistVideo, SubType};
use super::util::clean_filename;

// ── browser detection ─────────────────────────────────────────────────────────

const BROWSER_PRIORITY: &[(&str, &[&str])] = &[
    ("firefox", &["firefox"]),
    (
        "chrome",
        &["google-chrome", "google-chrome-stable", "chrome"],
    ),
    ("brave", &["brave-browser", "brave"]),
    ("edge", &["microsoft-edge", "msedge"]),
    ("chromium", &["chromium-browser", "chromium"]),
    ("opera", &["opera"]),
    ("vivaldi", &["vivaldi"]),
];

fn is_installed(exe: &str) -> bool {
    #[cfg(unix)]
    {
        Command::new("which")
            .arg(exe)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        Command::new("where")
            .arg(exe)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

// ── dependency checks ─────────────────────────────────────────────────────────
//
// These are pure checks: they report a problem, they don't print anything or
// exit the process themselves. Callers (the `commands` layer) decide how and
// when to surface the error, which makes this logic testable and reusable.

fn require_tool(exe: &str, display_name: &str) -> Result<(), String> {
    if is_installed(exe) {
        return Ok(());
    }
    Err(format!(
        "{display_name} not found — see the README for setup instructions."
    ))
}

/// Checks whether `yt-dlp` is on `PATH`.
pub fn require_yt_dlp() -> Result<(), String> {
    require_tool("yt-dlp", "yt-dlp")
}

/// Checks whether `ffmpeg` is on `PATH`.
pub fn require_ffmpeg() -> Result<(), String> {
    require_tool("ffmpeg", "ffmpeg")
}

/// Checks whether `deno` is on `PATH`. `yt-dlp` shells out to Deno
/// internally to run YouTube's signature/challenge scripts, so this is
/// checked alongside `yt-dlp` itself before any download begins.
pub fn require_deno() -> Result<(), String> {
    require_tool("deno", "deno")
}

// ── PO Token provider (soft check) ────────────────────────────────────────────
//
// Unlike the `require_*` checks above, this one is advisory, not mandatory:
// not every video needs a PO Token, so a missing provider isn't a reason to
// refuse to run. It exists purely so `commands` can warn about a likely
// cause upfront (see the README's Troubleshooting section) instead of the
// person only finding out after a confusing "no subtitles" result — the
// exact trial-and-error this project's own README documents.

/// Default host:port the `bgutil-ytdlp-pot-provider` HTTP server listens
/// on — matches both yt-dlp's own fallback when no `base_url` is set via
/// extractor-args, and what this project's README has people run it on.
const PO_TOKEN_PROVIDER_ADDR: &str = "127.0.0.1:4416";

/// Best-effort, near-instant check for whether a PO Token provider is
/// listening on its default port. `false` doesn't guarantee a download
/// will fail — plenty of videos don't need a PO Token at all — so this is
/// an upfront hint for `commands` to warn with, never a hard requirement.
pub fn po_token_provider_reachable() -> bool {
    PO_TOKEN_PROVIDER_ADDR
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .and_then(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok())
        .is_some()
}

/// Detects an installed browser to extract cookies from, in priority order
/// (see [`BROWSER_PRIORITY`]). Used when `--browser` isn't given.
pub fn detect_browser() -> Option<String> {
    for (browser_name, executables) in BROWSER_PRIORITY {
        if executables.iter().any(|exe| is_installed(exe)) {
            return Some(browser_name.to_string());
        }
    }
    None
}

// ── paths ─────────────────────────────────────────────────────────────────────

/// Returns the OS's Downloads folder, or `~/Downloads`, or `.` as a last
/// resort if neither can be determined.
pub fn get_downloads_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| {
        std::env::var("HOME")
            .map(|h| PathBuf::from(h).join("Downloads"))
            .unwrap_or_else(|_| PathBuf::from("."))
    })
}

/// Returns the user-chosen output directory (creating it if needed),
/// or falls back to the default Downloads folder.
pub fn resolve_output_dir(custom: &Option<PathBuf>) -> PathBuf {
    match custom {
        Some(p) => {
            let _ = std::fs::create_dir_all(p);
            p.clone()
        }
        None => get_downloads_dir(),
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Generates a short hex ID from the current time, used to namespace each
/// download's temp files (`temp_subs_{lang}_{temp_id}`) so concurrent or
/// retried downloads never collide.
pub fn temp_id() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{:016x}", d.as_nanos()))
        .unwrap_or_else(|_| "0".to_string())
}

fn build_yt_dlp(base_args: &[&str], browser: Option<&str>, url: &str, verbose: bool) -> Command {
    let mut cmd = Command::new("yt-dlp");
    cmd.args(base_args);
    if verbose {
        cmd.arg("-v");
    }
    if let Some(b) = browser {
        cmd.args(["--cookies-from-browser", b]);
    }
    // "--" marks the end of options, so `url` can never be misread as a yt-dlp flag.
    cmd.arg("--");
    cmd.arg(url);
    // Hidden by default — subtitleify has its own progress/error output.
    // In `--verbose` mode, stderr is explicitly inherited instead: leaving
    // it unset would still get silently captured, because `Command::output()`
    // pipes stdout *and* stderr by default whenever neither was configured
    // — only an explicit `Stdio::inherit()` makes it reach the terminal.
    // (`Command::status()`, used for the actual download, inherits by
    // default already; setting this explicitly here doesn't change that.)
    cmd.stderr(if verbose {
        Stdio::inherit()
    } else {
        Stdio::null()
    });
    cmd
}

fn opt_browser(browser: &str) -> Option<&str> {
    if browser.is_empty() {
        None
    } else {
        Some(browser)
    }
}

// ── video title ───────────────────────────────────────────────────────────────

fn try_get_title(url: &str, browser: Option<&str>, verbose: bool) -> Option<String> {
    let output = build_yt_dlp(
        &[
            "--get-title",
            "--no-check-certificates",
            "--sleep-requests",
            "2",
        ],
        browser,
        url,
        verbose,
    )
    .output()
    .ok()?;

    if !output.status.success() {
        return None;
    }
    let title = clean_filename(String::from_utf8_lossy(&output.stdout).trim());
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

/// Fetches a video's title via `yt-dlp --get-title`, cleaned for use as a
/// filename. Falls back to `"subtitles"` (with a warning on stderr) if the
/// title can't be fetched — a missing title shouldn't block the download.
pub fn get_video_title(url: &str, browser: &str, verbose: bool) -> String {
    if let Some(t) = try_get_title(url, opt_browser(browser), verbose) {
        return t;
    }

    eprintln!("  [warn] Could not fetch title; using 'subtitles'.");
    "subtitles".to_string()
}

// ── URL validation ────────────────────────────────────────────────────────────

/// Returns true if `url` looks like a YouTube video or playlist URL.
pub fn is_valid_youtube_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://"))
        && (lower.contains("youtube.com") || lower.contains("youtu.be"))
}

// ── playlist ──────────────────────────────────────────────────────────────────

/// Returns true if `url` contains a `list=` query parameter or `/playlist`
/// path — i.e. should be treated as a playlist rather than a single video.
pub fn is_playlist_url(url: &str) -> bool {
    url.contains("list=") || url.contains("/playlist")
}

fn try_fetch_playlist(
    url: &str,
    browser: Option<&str>,
    verbose: bool,
) -> Result<Playlist, Box<dyn std::error::Error>> {
    let output = build_yt_dlp(
        &["--flat-playlist", "-J", "--no-check-certificates"],
        browser,
        url,
        verbose,
    )
    .output()?;

    if !output.status.success() {
        return Err("yt-dlp failed to fetch playlist info".into());
    }

    let json: Value = serde_json::from_slice(&output.stdout)?;

    let title = clean_filename(
        json.get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("playlist"),
    );

    let videos: Vec<PlaylistVideo> = json
        .get("entries")
        .and_then(|v| v.as_array())
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|entry| {
            let raw_url = entry
                .get("webpage_url")
                .or_else(|| entry.get("url"))
                .and_then(|v| v.as_str())?;

            let video_url = if raw_url.starts_with("http") {
                raw_url.to_string()
            } else {
                format!("https://www.youtube.com/watch?v={raw_url}")
            };

            let video_title = clean_filename(
                entry
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("video"),
            );

            Some(PlaylistVideo {
                url: video_url,
                title: video_title,
            })
        })
        .collect();

    Ok(Playlist { title, videos })
}

/// Fetches a playlist's title and video list via `yt-dlp --flat-playlist`.
/// Entries missing both a URL and an ID are silently skipped rather than
/// failing the whole playlist.
pub fn fetch_playlist(
    url: &str,
    browser: &str,
    verbose: bool,
) -> Result<Playlist, Box<dyn std::error::Error>> {
    try_fetch_playlist(url, opt_browser(browser), verbose)
}

// ── language listing ──────────────────────────────────────────────────────────

fn query_sub_langs(
    url: &str,
    sub_type: &SubType,
    browser: Option<&str>,
    verbose: bool,
) -> Vec<String> {
    let output = build_yt_dlp(
        &["-j", "--ignore-errors", "--no-check-certificates"],
        browser,
        url,
        verbose,
    )
    .output();

    let stdout = match output {
        Ok(o) if !o.stdout.is_empty() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    let json: Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("  [warn] JSON parse error: {e}");
            return Vec::new();
        }
    };

    let field = match sub_type {
        SubType::Manual => "subtitles",
        SubType::Auto => "automatic_captions",
    };

    let map = match json.get(field).and_then(|v| v.as_object()) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let mut langs: Vec<String> = map
        .iter()
        .filter(|(_, formats)| {
            formats.as_array().is_some_and(|arr| {
                arr.iter()
                    .any(|f| f.get("ext").and_then(|e| e.as_str()) == Some("json3"))
            })
        })
        .map(|(k, _)| k.clone())
        .collect();

    langs.sort();
    langs
}

/// Lists every language code available for `url` and `sub_type`, restricted
/// to languages that offer a `json3`-format track (the only format
/// subtitleify knows how to parse). Returns an empty list — never an
/// error — if `url` has no captions of that type, or if `yt-dlp`/JSON
/// parsing fails.
pub fn list_available_subs(
    url: &str,
    sub_type: &SubType,
    browser: &str,
    verbose: bool,
) -> Vec<String> {
    query_sub_langs(url, sub_type, opt_browser(browser), verbose)
}

// ── downloading ───────────────────────────────────────────────────────────────

fn run_download_json3(
    url: &str,
    language: &str,
    sub_type: &SubType,
    temp_prefix: &str,
    browser: Option<&str>,
    verbose: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let sub_flag = match sub_type {
        SubType::Manual => "--write-subs",
        SubType::Auto => "--write-auto-subs",
    };
    let output_template = format!("{temp_prefix}.%(ext)s");

    let mut base: Vec<&str> = vec!["--skip-download"];
    // `--no-warnings` would silence exactly the kind of message `--verbose`
    // exists to surface (e.g. a missing PO Token), so it's only added when
    // verbose output wasn't requested.
    if !verbose {
        base.push("--no-warnings");
    }
    base.extend([
        sub_flag,
        "--sub-langs",
        language,
        "--sub-format",
        "json3",
        "-o",
        &output_template,
        "--no-check-certificates",
        "--sleep-requests",
        "3",
        "--extractor-retries",
        "5",
        "--retry-sleep",
        "exp=1:30",
    ]);

    let status = build_yt_dlp(&base, browser, url, verbose).status()?;

    if !status.success() {
        return Err("yt-dlp exited with a non-zero status".into());
    }

    let mut found: Vec<String> = Vec::new();
    for path in glob(&format!("{temp_prefix}*.json3"))?.flatten() {
        found.push(path.to_string_lossy().into_owned());
    }

    found
        .into_iter()
        .next()
        .ok_or_else(|| "No json3 file was downloaded.".into())
}

fn download_json3(
    url: &str,
    language: &str,
    sub_type: &SubType,
    temp_prefix: &str,
    browser: &str,
    verbose: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    run_download_json3(
        url,
        language,
        sub_type,
        temp_prefix,
        opt_browser(browser),
        verbose,
    )
}

/// Downloads one language's subtitles as json3 and returns the path to the
/// downloaded file, retrying up to `max_retries` times with exponential
/// back-off (2s, 4s, 8s, …) on failure — YouTube/`yt-dlp` failures are
/// often transient. Returns the last error seen if every attempt fails.
pub fn download_with_retry(
    url: &str,
    language: &str,
    sub_type: &SubType,
    temp_prefix: &str,
    browser: &str,
    max_retries: u32,
    verbose: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut last_err = String::new();

    for attempt in 0..=max_retries {
        if attempt > 0 {
            let secs = 2u64.pow(attempt);
            eprintln!("  [retry] attempt {attempt}/{max_retries} — waiting {secs}s …");
            std::thread::sleep(Duration::from_secs(secs));
        }
        match download_json3(url, language, sub_type, temp_prefix, browser, verbose) {
            Ok(path) => return Ok(path),
            Err(e) => last_err = e.to_string(),
        }
    }

    Err(last_err.into())
}
