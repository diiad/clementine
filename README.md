<p align="center"><img src="assets/logo.png" width="128" alt="Clémentine logo"></p>

<h1 align="center">Clémentine</h1>

<p align="center"><b>Drag a file. Hold a key. Drop it on the wheel.</b><br>
An open-source, offline file converter for macOS and Windows, written in Rust.</p>

---

Clémentine lives in the menu bar. Start dragging any file, from Finder or any other app, and hold a key. A wheel opens under the cursor:

| Hold | Wheel | Examples |
|---|---|---|
| **⇧ Shift** | **Convert** | HEIC → JPG, MOV → MP4, PDF → DOCX, XLSX → CSV, SRT → VTT, RAR → ZIP |
| **⌘ Command** | **Tools** | Compress, fit under 10 MB, trim, crop, resize, rotate, strip metadata, remove background, extract text (OCR), merge PDFs, split pages, extract audio, make a thumbnail |
| **⌃ Control** | **Actions** | AirDrop, Float, Copy, Move…, Copy path, open in Terminal |

Drop the file on a slice and the result appears next to the original. You can let go of the key once the wheel is open; dropping in the centre cancels.

Nothing is uploaded. Conversions run on your Mac, using Rust crates, macOS's own frameworks and ffmpeg (built into the app).

> Clémentine is an independent project inspired by the drag-and-hold workflow of [Tangerine for Mac](https://tangerineformac.com/). It isn't affiliated with Tangerine and shares no code with it.

## Features

- **Drag, hold, drop**: no Accessibility permission needed (see [How it works](#how-it-works)).
- **Keyboard and click mode**: press <kbd>⌥⌘T</kbd> to open the wheel for the Finder selection. Use arrow keys and <kbd>Enter</kbd>, <kbd>Tab</kbd> or <kbd>1</kbd>–<kbd>3</kbd> to switch wheels, and <kbd>Esc</kbd> to close.
- **Multiple files**: the wheel shows only the conversions that apply to every selected file, plus merge tools when they make sense.
- **On-device AI** through Apple's Vision framework: *Remove BG* cuts the subject out of a photo into a transparent PNG (macOS 14+), and *Extract text* / *OCR text* reads the text of images and scanned PDFs. Nothing is uploaded.
- **Float shelf**: park files in a small always-on-top window and drag them out later.
- **Trim and crop**: *Trim…* opens an editor with the video or audio and a range to keep (I/O mark it at the playhead), *Crop…* one with the picture and a frame to drag, free or 1:1, 16:9, 4:3, 9:16.
- **Finder integration**: right-click any file → *Open With* → *Clémentine* opens the wheel for it, as does `open -a Clémentine <files>` (handy in Shortcuts).
- **Recipes**: chain steps into one slice of the Tools wheel, e.g. *Fit 1080 px → JPG*. Build them in Settings → Recipes; only the last step's files are kept.
- **Fit a size**: *≤ 10 MB* re-encodes a video (two-pass), audio or image to fit under a size, for Discord or email. The target is set in Settings.
- **Video presets**: *Standard* follows the quality slider, *Web* caps at 1080p and keeps files light, *Max* is near lossless.
- **Notifications** when a long job finishes, with an **Open** button; clicking the notification shows the result in Finder.
- **Settings**: choose the trigger keys, where results go (next to the original, Downloads or a folder you pick), quality, PDF/SVG resolution and launch at login.
- **Haptic feedback**: the trackpad ticks as you move across slices. Light uses AppKit; Medium and Strong drive the trackpad actuator through the private MultitouchSupport framework (loaded at runtime, falling back to AppKit).
- **English or French**: the interface follows the macOS language, or pick one in Settings.
- **CLI**: the same engine runs from the command line as `clementine`.

### Formats

| Family | Reads | Writes |
|---|---|---|
| Images | JPG, PNG, WebP, HEIC/HEIF, AVIF, TIFF, BMP, GIF, ICO, TGA, SVG | JPG, PNG, WebP, HEIC, AVIF, TIFF, GIF, BMP, ICO, ICNS, PDF |
| Audio | MP3, M4A/AAC, WAV, FLAC, OGG, Opus, AIFF, WMA, CAF | MP3, M4A, AAC, WAV, FLAC, OGG, Opus, AIFF, WMA |
| Video | MP4, MOV, M4V, MKV, WebM, AVI, WMV, FLV, MPEG, 3GP, TS | MP4, MOV, M4V, MKV, WebM, AVI, WMV, GIF, plus MP3/M4A/WAV/FLAC soundtrack |
| PDF | PDF | PNG/JPG/TIFF (one per page), DOCX, ODT, RTF, HTML, TXT, Markdown, EPUB |
| Documents | DOCX, DOC, ODT, RTF, TXT, HTML, Markdown, EPUB, WebArchive (Pages with LibreOffice) | DOCX, ODT, RTF, PDF, HTML, TXT, Markdown, EPUB |
| Slides | PPTX, PPT, ODP, Keynote (*requires LibreOffice*) | PDF, PPTX, ODP |
| Spreadsheets | XLSX, XLSM, XLS, XLSB, ODS, CSV, TSV | XLSX, CSV, TSV, JSON, HTML, Markdown, PDF (*LibreOffice*) |
| Subtitles | SRT, VTT | SRT, VTT, TXT |
| Archives | ZIP, RAR, 7z, TAR, TGZ, GZ, BZ2, XZ, ISO | Extract, ZIP, TAR.GZ |

Any file or folder can also be zipped.

## Install

**Requirements:** macOS 13 or later. Development so far has been on Apple silicon.

**With Homebrew** (recommended):

```sh
brew install --cask diiad/tap/clementine
brew install --cask libreoffice             # optional: slides, better documents
```

The command-line tool alone: `brew install diiad/tap/clementine-cli`.

**Or by hand:**

1. Download the latest `.dmg` from [Releases](../../releases) and drag **Clémentine** to Applications.
   The app isn't notarized yet: the first time, right-click it in Applications and choose **Open** (or run `xattr -dr com.apple.quarantine /Applications/Clémentine.app`).
2. Optional: install [LibreOffice](https://www.libreoffice.org/) (`brew install --cask libreoffice`) for slides, spreadsheet → PDF, and higher-fidelity documents: DOCX/ODT/PDF output is then laid out by LibreOffice Writer (real tables, lists, images) instead of textutil + CoreText.

### Windows

Windows 10 and 11. Three ways to open the wheel:

- **Drag** files from the desktop or File Explorer and hold <kbd>Shift</kbd> (convert), <kbd>Alt</kbd> (tools) or <kbd>Ctrl</kbd> (actions), as on macOS. Dropping on the wheel never moves the original.
- **Right-click** files in File Explorer → **Open with Clémentine** (under *Show more options* on Windows 11).
- Or select files in File Explorer and press <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>T</kbd>, changeable in Settings.

Install the `.exe` setup from [Releases](../../releases). ffmpeg is built in; LibreOffice (documents and slides) installs from **Settings → Engines → Install**, or with `winget install TheDocumentFoundation.LibreOffice`. The setup isn't signed yet: on the SmartScreen warning, choose **More info → Run anyway**.

| Available | Not yet on Windows |
|---|---|
| Images (JPG, PNG, WebP, AVIF output, SVG…), HEIC/AVIF input with Microsoft's *HEIF Image Extensions* / *AV1 Video Extension*, audio, video, documents and slides via LibreOffice, spreadsheets, subtitles, archives; PDF → images/text/documents, merge, split, rotate, first page, images → PDF (PDFium); OCR of images and scanned PDFs (Windows OCR, needs a language with OCR in Settings → Language); trim and crop; Float, Copy, Move, Copy path, Terminal | PDF compression, HEIC/ICNS output, background removal, AirDrop, haptics |

### Updates

Both apps check for a new version at launch and every 6 hours, and ask before installing it (Settings → General). Updates are signed; the feed lives in the [`updater`](../../releases/tag/updater) release.

### Build from source

```sh
git clone https://github.com/diiad/clementine
cd clementine
cargo install tauri-cli --version "^2" --locked

# run the app in development
cd app/src-tauri && cargo tauri dev

# or bundle Clémentine.app + .dmg into target/release/bundle/
cargo tauri build

# no GUI session (SSH, locked screen)? The styled DMG needs Finder;
# this makes a plain one with hdiutil instead
../../scripts/make-dmg.sh
```

## Command line

```sh
cargo install --path crates/cli

clementine list photo.heic                   # what can be done with a file
clementine convert jpg *.heic                # convert:jpg on each file
clementine convert mp4 clip.mov -o ~/Desktop # write results elsewhere
clementine run tool:merge a.pdf b.pdf        # any operation id from `list`
clementine run tool:compress video.mov --json
clementine run tool:fit-size clip.mov --target-mb 25   # fit under 25 MB
clementine convert mp4 clip.mkv --preset web           # standard, web or max
clementine chain -s tool:resize-1080,convert:jpg *.png # a recipe: steps in order
```

### MCP server

`clementine mcp` serves the engine over the [Model Context Protocol](https://modelcontextprotocol.io) (stdio), so AI agents can convert files on your machine. Tools: `list_operations`, `list_catalog`, `run_operation` and `run_recipe`.

```sh
claude mcp add clementine -- clementine mcp     # Claude Code
```

For Claude Desktop, add to `claude_desktop_config.json`:

```json
{ "mcpServers": { "clementine": { "command": "/opt/homebrew/bin/clementine", "args": ["mcp"] } } }
```

## How it works

```
app (Tauri 2)                                clementine-core
├─ watcher.rs  mouse buttons, modifier       ├─ registry  operations() / run()
│              flags, drag pasteboard        ├─ image     image, resvg, libwebp, sips
├─ wheel.rs    slice layout + drop hit-test  ├─ media     ffmpeg
├─ main.rs     tray, windows, job runner ──▶ ├─ pdf       PDFKit, CoreGraphics, CoreText
└─ ui/         wheel (SVG), settings, float  ├─ document  textutil, LibreOffice
                                             ├─ sheet     calamine, rust_xlsxwriter
                                             ├─ vision    Vision (OCR, subject mask)
                                             └─ archive   bsdtar (libarchive)
```

- **Detecting the drag.** A background thread reads `+[NSEvent pressedMouseButtons]`, `+[NSEvent modifierFlags]` and the change count of the drag pasteboard (`NSPasteboardNameDrag`), which increases whenever any app starts a drag. If the mouse is down, the count has changed and the pasteboard holds file URLs, a drag is in progress. None of these APIs needs Accessibility or Input Monitoring permission.
- **The wheel** is a transparent, borderless window placed at the pop-up-menu level, centred on the cursor. The native drop events give the drop position. Rust hit-tests it against the same geometry the SVG is drawn from.
- **Conversions** run on a worker thread and report progress (ffmpeg's `-progress` output, page counts) back to the wheel's hub.

## Project layout

```
crates/core   conversion engine (library, unit tested)
crates/cli    `clementine` command
app/src-tauri menu-bar app
app/ui        HTML/CSS/JS for the wheel, settings and float windows
assets        logo sources
```

## Contributing

Issues and pull requests are welcome. Before you open a PR, run:

```sh
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Good first issues:

- PDF compression and background removal on Windows.
- Linux support. The core is mostly portable; the PDF and drag-watcher modules are platform-specific.

## License

MIT. See [LICENSE](LICENSE). ffmpeg and LibreOffice run as separate programs under their own licenses; the apps bundle a static ffmpeg (GPL v3, license and source location next to it, see [docs/RELEASING.md](docs/RELEASING.md)). The Windows build ships [PDFium](https://pdfium.googlesource.com/pdfium/) (BSD-3-Clause, prebuilt by [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries)).
