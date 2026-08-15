# subtitleify

**A fast CLI tool to download, clean, convert, and embed YouTube subtitles.**

Download subtitles directly from YouTube, remove formatting noise and HTML artifacts, export them as **VTT** or **SRT**, and merge them into videos with proper language metadata using **ffmpeg**.

**100% local • No tracking • No ads • No cloud • Script-friendly**

---

## Table of Contents

- [Why subtitleify Exists](#why-subtitleify-exists)
- [Before / After](#before--after)
- [Requirements & Troubleshooting](#requirements--troubleshooting)
  - [Installing yt-dlp](#installing-yt-dlp)
  - [Installing ffmpeg](#installing-ffmpeg)
  - [Installing Deno](#installing-deno)
  - ["No PO Token provider detected" heads-up](#no-po-token-provider-detected-at-1270014416-every-time-you-run-a-command)
  - ["No subtitles found" even though captions exist](#no-subtitles-found-even-though-the-video-clearly-has-captions)
  - [The PO Token fix stopped working again](#the-po-token-fix-was-working-then-suddenly-stopped-again)
  - ["language 'xx' not available"](#language-xx-not-available-for-a-language-you-know-exists)
- [Installation](#installation)
  - [Build from source](#build-from-source)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
- [Quick Start](#quick-start)
- [Download Mode](#download-mode)
  - [Single video](#single-video)
  - [Playlist](#playlist)
  - [Checking available languages first](#checking-available-languages-first)
  - [Mixing manual and auto in one command](#mixing-manual-and-auto-in-one-command)
- [Merge Mode](#merge-mode)
  - [Folder mode](#folder-mode)
  - [Single file mode](#single-file-mode)
- [Browser Selection](#browser-selection)
- [Command Line Options](#command-line-options)
  - [subtitleify download](#subtitleify-download)
  - [subtitleify languages](#subtitleify-languages)
  - [subtitleify merge folder](#subtitleify-merge-folder)
  - [subtitleify merge single](#subtitleify-merge-single)
- [License](#license)

---

## Why subtitleify Exists

Raw subtitle files pulled straight from YouTube are rarely usable as-is. subtitleify was built to fix three specific problems:

- **Clean text, no noise** — raw subtitle files often carry HTML tags and other formatting artifacts mixed into the actual text. subtitleify strips all of that out, leaving just the subtitle content itself.
- **No repeated lines** — YouTube's auto-generated captions build up each line word by word, re-sending the growing sentence over and over (see [Before / After](#before--after) below). subtitleify collapses this into a clean, non-repeating subtitle file.
- **Proper RTL support** — subtitles in right-to-left languages (Arabic, Hebrew, etc.) are displayed and handled correctly, instead of coming out visually broken or reordered.

The result is a subtitle file that's easy to read, easy to edit by hand, and easy to merge into your video however you like.

---

## Before / After

YouTube's auto-generated captions build up each line word by word, re-sending the growing sentence over and over until it's replaced by the next one — timestamps and all, right in the raw `.vtt` file:

**Before (raw YouTube captions):**

```vtt
WEBVTT

00:00:00.000 --> 00:00:02.000
Hello everyone and welcome back

00:00:02.000 --> 00:00:04.500
Hello everyone and welcome back
to the channel

00:00:04.500 --> 00:00:07.000
to the channel
how are you doing today
```

**After (`subtitleify download`):**

```vtt
WEBVTT

00:00:00.000 --> 00:00:02.000
Hello everyone and welcome back

00:00:02.000 --> 00:00:04.500
to the channel

00:00:04.500 --> 00:00:07.000
how are you doing today
```

Each cue appears once, in order, with no repeated text — a normal, readable subtitle file instead of a rolling transcript.

---

## Requirements & Troubleshooting

subtitleify depends on the following tools:

|Dependency|Purpose|Required for|
|---|---|---|
|yt-dlp|Fetches subtitle data from YouTube|Download|
|Deno|JavaScript runtime used internally by yt-dlp to solve YouTube's signature/challenge scripts|Download|
|ffmpeg|Embeds subtitles into video files|Merge|

subtitleify itself doesn't call Deno directly — it's a dependency of `yt-dlp`, which needs a real JS runtime to keep working as YouTube's anti-bot challenges evolve. Because of this, subtitleify only checks for Deno when you run it in **download** mode; merge mode only requires `ffmpeg`.

### Installing yt-dlp

Install it with `pipx` — this keeps it up to date and out of the way of your system Python. Avoid plain `pip install yt-dlp`; most distros now block `pip` from installing outside a virtual environment ([PEP 668](https://peps.python.org/pep-0668/)), and `pipx` is the supported way to install Python CLI tools system-wide without hitting that.

|Platform|pipx|
|---|---|
|Debian / Ubuntu|`sudo apt install pipx`|
|Fedora / RHEL|`sudo dnf install pipx`|
|Arch Linux|`sudo pacman -S python-pipx`|
|openSUSE|`sudo zypper install python3-pipx`|

```bash
pipx install yt-dlp
```

YouTube extraction can start failing if yt-dlp falls behind upstream, so keep it updated:

```bash
pipx upgrade yt-dlp
```

### Installing ffmpeg

|Platform|Command|
|---|---|
|Debian / Ubuntu|`sudo apt install ffmpeg`|
|Fedora / RHEL|`sudo dnf install ffmpeg`|
|Arch Linux|`sudo pacman -S ffmpeg`|
|openSUSE|`sudo zypper install ffmpeg`|

**Note:** on Fedora, plain `dnf install ffmpeg` (or the `ffmpeg-free` package) is enough for subtitleify — merging only stream-copies your existing video/audio and re-encodes the subtitle track, so the patented codecs behind RPM Fusion's full build aren't needed.

### Installing Deno

```bash
curl -fsSL https://deno.land/install.sh | sh
```

### "No PO Token provider detected at 127.0.0.1:4416" every time you run a command

`download` and `languages` both do a quick, near-instant check on startup for a PO Token provider listening on its default port, and print this as a heads-up if none is found — **before** doing any real work, rather than only after a confusing "no subtitles" result partway through. It's informational, not an error: subtitleify keeps going either way, since plenty of videos don't need a PO Token at all. If you keep seeing it and hit actual failures, follow the section below; if your downloads are working fine, it's safe to ignore.

### "No subtitles found" even though the video clearly has captions

As of 2026, YouTube increasingly requires a **PO Token** (Proof-of-Origin Token) before it will hand over subtitle data — without one, it can silently return an empty result instead of a clear error.

**1. Confirm it's a PO Token issue.** Re-run the same command with `--verbose` and look for either of these in yt-dlp's own output:

```bash
subtitleify download --url "..." --type auto en --format srt --verbose
```

```
[debug] [youtube] [pot] PO Token Providers: none
...
There are missing subtitles languages because a PO token was not provided.
```

**2. Install a PO Token provider plugin**, so yt-dlp can generate tokens automatically. Which command to run depends on how yt-dlp itself is installed — check first:

```bash
pipx list 2>/dev/null | grep -q yt-dlp && echo "pipx" || echo "not-pipx"
```

```bash
# yt-dlp installed via pipx
pipx inject yt-dlp bgutil-ytdlp-pot-provider

# yt-dlp installed via plain pip
pip install -U bgutil-ytdlp-pot-provider --break-system-packages
```

**3. Run the token-generation server.** Docker is the simplest option — no Node.js or extra Python setup needed on the host:

```bash
docker run --name bgutil-provider -d --restart unless-stopped -p 4416:4416 brainicism/bgutil-ytdlp-pot-provider
```

`--restart unless-stopped` keeps the container coming back after a reboot — without it you'd need to `docker start bgutil-provider` by hand every time.

**4. Verify it worked.** Re-run the same `--verbose` command and check the `PO Token Providers:` line — it should no longer say `none`:

```
[debug] [youtube] [pot] PO Token Providers: bgutil:http-1.3.1 (external), ...
[youtube] [pot:bgutil:http] Generating a gvs PO Token for web_safari client via bgutil HTTP server
[debug] [youtube] ...: Retrieved a gvs PO Token for web_safari client
```

Subtitles should now download normally. See the [yt-dlp PO Token Guide](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide) if this doesn't resolve it — YouTube's requirements here shift over time.

### The PO Token fix was working, then suddenly stopped again

If subtitleify goes back to failing with `--verbose` showing something like:

```
WARNING: [youtube] [pot:bgutil:http] Error reaching GET http://127.0.0.1:4416/ping (caused by TransportError).
```

the plugin is still registered fine — the token *server* itself just isn't running anymore. Check its status:

```bash
docker ps -a | grep bgutil-provider
```

If it shows `Exited (137)`, that's almost always the Linux kernel's OOM killer, not a bug in the container — the host briefly ran low on memory and the kernel picked this process to kill. Bring it back and make it self-healing so this doesn't need manual attention again:

```bash
docker update --restart=unless-stopped bgutil-provider
docker start bgutil-provider
```

If it keeps recurring, check how much memory headroom the host actually has, and cap the container so a memory spike can't take the rest of the system down with it:

```bash
free -h
docker update --memory=512m --memory-swap=512m bgutil-provider
```

### "language 'xx' not available" for a language you know exists

`--type`'s language list has to match the exact code yt-dlp reports, which isn't always the plain code you'd expect — a video might only expose `en-US` rather than `en`, for instance. List what's actually available and use that exact code:

```bash
subtitleify languages --url "..."
```

---

## Installation

### Build from source

```bash
git clone https://github.com/AbuKaram01/subtitleify
cd subtitleify
cargo build --release
sudo cp target/release/subtitleify /usr/local/bin/
```

### Debian / Ubuntu (.deb)

```bash
cargo install cargo-deb
cargo deb
sudo dpkg -i target/debian/subtitleify_*.deb
```

### Fedora / RHEL / openSUSE (.rpm)

```bash
cargo install cargo-generate-rpm
cargo build --release
cargo generate-rpm
sudo rpm -i target/generate-rpm/subtitleify-*.rpm
```

---

# Quick Start

Every command is driven entirely by flags — there is no interactive mode. `subtitleify --help` shows the full command list, and `subtitleify <command> --help` shows a command's flags.

---

# Download Mode

### Single video

```bash
subtitleify download \
    --url "https://youtube.com/watch?v=..." \
    --type auto ar,en \
    --format srt \
    --output "/path/to/output"
```

### Playlist

```bash
subtitleify download \
    --url "https://youtube.com/playlist?list=..." \
    --type auto ar,en \
    --format vtt \
    --output "/path/to/output"
```

### Checking available languages first

Not sure what's available before writing a `--type` language list? List every manual and auto-generated language for a video without downloading anything:

```bash
subtitleify languages --url "https://youtube.com/watch?v=..."
```

### Mixing manual and auto in one command

`--type` takes a type (`manual`/`auto`) followed by a comma-separated language list for that type, and can be repeated to mix types in a single command instead of running `download` twice:

```bash
# en as manual; ar and fr as auto — all in one command
subtitleify download \
    --url "https://youtube.com/watch?v=..." \
    --type manual en \
    --type auto ar,fr \
    --format srt
```

The same language can appear under more than one type — each is downloaded and saved as a separate file, named `name (manual).ext` / `name (auto).ext` so they don't collide:

```bash
# ar as both manual and auto
subtitleify download \
    --url "https://youtube.com/watch?v=..." \
    --type manual ar \
    --type auto ar \
    --format srt
```

---

# Merge Mode

### Folder mode

```bash
subtitleify merge folder \
    --videos-dir "/path/to/videos" \
    --subs-dir "/path/to/subtitles" \
    --output "/path/to/output"
```

### Single file mode

```bash
subtitleify merge single \
    --video "/path/to/video.mkv" \
    --sub "/path/to/sub_ar.srt" \
    --sub "/path/to/sub_en.srt" \
    --output "/path/to/output"
```

---

# Browser Selection

By default, `subtitleify download` automatically detects an installed browser (used for cookie authentication) using the following priority:

```
Firefox → Chrome → Brave → Edge → Chromium → Opera → Vivaldi
```

Merge mode doesn't need a browser at all. To override automatic detection for downloads:

```bash
subtitleify download --browser brave --url "..." --type auto ar --format srt
```

---

# Command Line Options

`--verbose` (`-v`) works with every subcommand below — before or after it — and shows yt-dlp's/ffmpeg's own output instead of hiding it:

```bash
subtitleify --verbose download --url "..." --type auto en --format srt
# or, equivalently:
subtitleify download --url "..." --type auto en --format srt --verbose
```

This is the fastest way to see *why* something silently comes back empty — e.g. a missing PO Token, a bot-check challenge, or SABR-only formats — without re-running yt-dlp by hand. See [Requirements & Troubleshooting](#requirements--troubleshooting) above.

### `subtitleify download`

|Flag|Short|Description|
|---|---|---|
|`--url`||YouTube video or playlist URL|
|`--type`|`-t`|`TYPE LANGS` — `manual` or `auto`, followed by comma-separated language codes for that type. Repeatable to mix types (see [above](#mixing-manual-and-auto-in-one-command))|
|`--format`|`-f`|`vtt` or `srt`|
|`--output`|`-o`|Output folder|
|`--browser`|`-b`|Browser used for cookie authentication|
|`--verbose`|`-v`|Show yt-dlp's own output|

`--url`, `--type`, and `--format` are all required.

### `subtitleify languages`

|Flag|Short|Description|
|---|---|---|
|`--url`||YouTube video or playlist URL|
|`--browser`|`-b`|Browser used for cookie authentication|
|`--verbose`|`-v`|Show yt-dlp's own output|

Lists every available manual and auto-generated language for `--url` and exits — no download, no other flags needed.

### `subtitleify merge folder`

|Flag|Short|Description|
|---|---|---|
|`--videos-dir`||Videos folder path|
|`--subs-dir`||Subtitles folder path|
|`--output`|`-o`|Output folder|
|`--verbose`|`-v`|Show ffmpeg's own output|

### `subtitleify merge single`

|Flag|Short|Description|
|---|---|---|
|`--video`||Video file path|
|`--sub`||Subtitle file path (repeatable, at least one required)|
|`--output`|`-o`|Output folder — defaults to alongside the source video|
|`--verbose`|`-v`|Show ffmpeg's own output|

---

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).

---

[⬆ Back to top](#subtitleify)
