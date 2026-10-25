//! `clementine` — the conversion engine from the command line.
//!
//! ```text
//! clementine list photo.heic            # what can I do with this file?
//! clementine convert webp *.png         # convert:webp on every file
//! clementine run tool:merge a.pdf b.pdf # any operation id from `list`
//! clementine chain -s tool:resize-1080,convert:jpg *.png  # a recipe
//! ```

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use clementine_core::{operations, run, run_recipe, Category, Options, VideoPreset};
use std::io::Write;
use std::path::PathBuf;

mod mcp;

#[derive(Parser)]
#[command(
    name = "clementine",
    version,
    about = "Convert files locally: images, audio, video, PDF, documents, archives"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Write results to this folder instead of next to the source.
    #[arg(short, long, global = true)]
    out: Option<PathBuf>,
    /// Lossy quality, 1-100.
    #[arg(short, long, global = true, default_value_t = 85)]
    quality: u8,
    /// Resolution for PDF/SVG rasterisation.
    #[arg(long, global = true, default_value_t = 300)]
    dpi: u32,
    /// Video encoding preset: standard, web (≤ 1080p, lighter) or max.
    #[arg(long, global = true, default_value = "standard", value_parser = ["standard", "web", "max"])]
    preset: String,
    /// Size `tool:fit-size` aims under, in MB.
    #[arg(long, global = true, default_value_t = 10.0)]
    target_mb: f64,
    /// Print results as JSON.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// List the operations available for the given files.
    List { files: Vec<PathBuf> },
    /// Convert files to another format (e.g. `clementine convert jpg a.heic`).
    Convert { format: String, files: Vec<PathBuf> },
    /// Run any operation id, e.g. `tool:compress` or `convert:mp3`.
    Run { op: String, files: Vec<PathBuf> },
    /// Run several operations in a row, each on the previous one's results
    /// (e.g. `clementine chain -s tool:resize-1080,convert:jpg a.png`).
    Chain {
        /// Operation ids, in order (comma-separated or repeated).
        #[arg(short, long = "step", value_delimiter = ',', required = true)]
        steps: Vec<String>,
        files: Vec<PathBuf>,
    },
    /// Serve the engine to AI agents over the Model Context Protocol (stdio).
    Mcp,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let opts = Options {
        out_dir: cli.out.clone(),
        quality: cli.quality,
        dpi: cli.dpi,
        video_preset: match cli.preset.as_str() {
            "web" => VideoPreset::Web,
            "max" => VideoPreset::Max,
            _ => VideoPreset::Standard,
        },
        target_mb: cli.target_mb,
    };
    let (steps, files) = match cli.command {
        Command::List { files } => return list(&files, cli.json),
        Command::Mcp => return mcp::serve(),
        Command::Convert { format, files } => (
            vec![format!(
                "convert:{}",
                format.trim_start_matches('.').to_lowercase()
            )],
            files,
        ),
        Command::Run { op, files } => (vec![op], files),
        Command::Chain { steps, files } => (steps, files),
    };
    if files.is_empty() {
        bail!("no input files");
    }
    for f in &files {
        if !f.exists() {
            bail!("{} does not exist", f.display());
        }
    }
    let show_progress = !cli.json && std::io::IsTerminal::is_terminal(&std::io::stderr());
    let progress = move |p: f32| {
        if show_progress {
            eprint!("\r{:>3.0}%", p * 100.0);
            let _ = std::io::stderr().flush();
        }
    };
    let result = match steps.as_slice() {
        [op] => run(op, &files, &opts, &progress),
        _ => run_recipe(&steps, &files, &opts, &progress),
    };
    if show_progress {
        eprint!("\r    \r");
    }
    let outputs = result?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&outputs)?);
    } else {
        for o in outputs {
            println!("{}", o.display());
        }
    }
    Ok(())
}

fn list(files: &[PathBuf], json: bool) -> Result<()> {
    if files.is_empty() {
        bail!("no input files");
    }
    let groups = [
        ("Convert  (⇧ Shift)", Category::Convert),
        ("Tools    (⌘ Command)", Category::Tool),
        ("Actions  (⌃ Control)", Category::Action),
    ];
    if json {
        let all: Vec<_> = groups
            .iter()
            .flat_map(|(_, c)| operations(files, *c))
            .collect();
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    for (title, cat) in groups {
        let ops = operations(files, cat);
        if ops.is_empty() {
            continue;
        }
        println!("{title}");
        for op in ops {
            println!("  {:<22} {}", op.id, op.label);
        }
    }
    Ok(())
}
