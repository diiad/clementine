//! Audio and video through ffmpeg.

use super::find_tool;
use crate::kind::{detect, ext, Kind};
use crate::{output_path, Error, Options, Progress, Result};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HINT: &str = if cfg!(windows) {
    "Install it with `winget install Gyan.FFmpeg`, or set CLEMENTINE_FFMPEG."
} else {
    "Install it with `brew install ffmpeg`, or set CLEMENTINE_FFMPEG."
};

fn ffmpeg() -> Result<PathBuf> {
    find_tool("ffmpeg").ok_or(Error::MissingTool("ffmpeg", HINT))
}

/// Duration in seconds, read from ffmpeg's banner (avoids requiring ffprobe).
pub fn duration(input: &Path) -> Option<f64> {
    let out = Command::new(ffmpeg().ok()?)
        .args(["-hide_banner", "-i"])
        .arg(input)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("Duration:"))?;
    let stamp = line
        .trim_start()
        .strip_prefix("Duration:")?
        .split(',')
        .next()?
        .trim();
    parse_timestamp(stamp)
}

fn parse_timestamp(s: &str) -> Option<f64> {
    let mut parts = s.split(':').rev();
    let secs: f64 = parts.next()?.parse().ok()?;
    let mins: f64 = parts.next().unwrap_or("0").parse().ok()?;
    let hours: f64 = parts.next().unwrap_or("0").parse().ok()?;
    Some(hours * 3600.0 + mins * 60.0 + secs)
}

/// Run ffmpeg with `args` (inputs and filters, not the output) writing `out`,
/// reporting progress against `total` seconds.
fn run_ffmpeg(args: &[String], out: &Path, total: Option<f64>, progress: Progress) -> Result<()> {
    let mut cmd = Command::new(ffmpeg()?);
    cmd.args([
        "-hide_banner",
        "-nostdin",
        "-y",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
        "-nostats",
    ])
    .args(args)
    .arg(out)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| Error::ToolFailed {
        tool: "ffmpeg".into(),
        message: e.to_string(),
    })?;
    let mut stderr = child.stderr.take().expect("piped");
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            if let (Some(us), Some(total)) = (line.strip_prefix("out_time_us="), total) {
                if let Ok(us) = us.parse::<f64>() {
                    progress(((us / 1e6) / total).clamp(0.0, 0.99) as f32);
                }
            }
        }
    }
    let status = child.wait()?;
    let stderr = err_reader.join().unwrap_or_default();
    if !status.success() {
        let _ = std::fs::remove_file(out);
        let message = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("conversion failed");
        return Err(Error::ToolFailed {
            tool: "ffmpeg".into(),
            message: message.trim().into(),
        });
    }
    progress(1.0);
    Ok(())
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// Make width and height even, which H.264 / VP9 with yuv420p require.
const EVEN: &str = "scale=trunc(iw/2)*2:trunc(ih/2)*2";

fn audio_codec(target: &str) -> Vec<String> {
    s(match target {
        "mp3" => &["-c:a", "libmp3lame", "-q:a", "2"],
        "m4a" | "aac" => &["-c:a", "aac", "-b:a", "192k"],
        "wav" => &["-c:a", "pcm_s16le"],
        "flac" => &["-c:a", "flac"],
        "ogg" => &["-c:a", "libopus", "-b:a", "160k"],
        "opus" => &["-c:a", "libopus", "-b:a", "128k"],
        "aiff" => &["-c:a", "pcm_s16be"],
        "wma" => &["-c:a", "wmav2", "-b:a", "192k"],
        _ => &[],
    })
}

fn video_codec(target: &str, crf: u8) -> Vec<String> {
    let crf = crf.to_string();
    let mut v = match target {
        "mp4" | "m4v" | "mov" | "mkv" => s(&[
            "-c:v", "libx264", "-preset", "medium", "-crf", &crf, "-pix_fmt", "yuv420p", "-vf",
            EVEN, "-c:a", "aac", "-b:a", "160k",
        ]),
        "webm" => s(&[
            "-c:v",
            "libvpx-vp9",
            "-crf",
            &(crf.parse::<u8>().unwrap_or(23) + 10).to_string(),
            "-b:v",
            "0",
            "-row-mt",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "libopus",
            "-b:a",
            "128k",
        ]),
        "avi" => s(&[
            "-c:v",
            "mpeg4",
            "-q:v",
            "3",
            "-vf",
            EVEN,
            "-c:a",
            "libmp3lame",
            "-q:a",
            "3",
        ]),
        "wmv" => s(&[
            "-c:v", "wmv2", "-b:v", "3M", "-vf", EVEN, "-c:a", "wmav2", "-b:a", "192k",
        ]),
        _ => Vec::new(),
    };
    if matches!(target, "mp4" | "m4v" | "mov") {
        v.extend(s(&["-movflags", "+faststart"]));
    }
    v
}

fn input_args(input: &Path) -> Vec<String> {
    vec!["-i".into(), input.to_string_lossy().into_owned()]
}

pub fn convert(input: &Path, target: &str, opts: &Options, progress: Progress) -> Result<PathBuf> {
    let out = output_path(input, "", target, opts);
    let total = duration(input);
    let mut args = input_args(input);
    let audio_only = matches!(
        target,
        "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "opus" | "aiff" | "wma"
    );
    if audio_only {
        args.push("-vn".into());
        args.extend(audio_codec(target));
    } else if target == "gif" {
        args.extend(s(&[
            "-vf",
            "fps=12,scale='min(720,iw)':-2:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
            "-loop",
            "0",
        ]));
    } else {
        // Same codecs in a different container: try a lossless remux first.
        if matches!(target, "mov" | "mkv" | "mp4" | "m4v") && is_h264_aac(input) {
            let mut remux = input_args(input);
            remux.extend(s(&["-map", "0", "-c", "copy"]));
            if run_ffmpeg(&remux, &out, total, progress).is_ok() {
                return Ok(out);
            }
        }
        args.extend(video_codec(target, crf_for(opts.quality)));
    }
    run_ffmpeg(&args, &out, total, progress)?;
    Ok(out)
}

/// Map a 1..=100 quality to an x264 CRF (85 → 20, 50 → 28).
fn crf_for(quality: u8) -> u8 {
    (38.0 - quality.clamp(1, 100) as f32 * 0.21).round() as u8
}

fn is_h264_aac(input: &Path) -> bool {
    let Ok(ff) = ffmpeg() else { return false };
    let Ok(out) = Command::new(ff)
        .args(["-hide_banner", "-i"])
        .arg(input)
        .output()
    else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stderr);
    let video_ok = text
        .lines()
        .filter(|l| l.contains("Video:"))
        .all(|l| l.contains("h264"));
    let audio_ok = text
        .lines()
        .filter(|l| l.contains("Audio:"))
        .all(|l| l.contains("aac"));
    video_ok && audio_ok && text.contains("Video:")
}

pub fn tool(input: &Path, name: &str, opts: &Options, progress: Progress) -> Result<PathBuf> {
    let e = ext(input);
    let is_video = detect(input) == Kind::Video;
    let total = duration(input);
    let mut args = input_args(input);
    let (suffix, target): (&str, String) = match (name, is_video) {
        ("compress", true) => {
            args.extend(video_codec("mp4", 30));
            ("-compressed", "mp4".into())
        }
        ("compress", false) => {
            args.extend(s(&["-vn", "-c:a", "libmp3lame", "-b:a", "128k"]));
            ("-compressed", "mp3".into())
        }
        ("resize-720" | "resize-1080", true) => {
            let h = if name == "resize-720" { 720 } else { 1080 };
            let mut v = video_codec("mp4", crf_for(opts.quality));
            let i = v
                .iter()
                .position(|a| a == "-vf")
                .expect("mp4 args have -vf");
            v[i + 1] = format!("scale=-2:'min({h},ih)'");
            args.extend(v);
            (if h == 720 { "-720p" } else { "-1080p" }, "mp4".into())
        }
        ("extract-audio", true) => {
            args.push("-vn".into());
            args.extend(audio_codec("m4a"));
            ("", "m4a".into())
        }
        ("mute", true) => {
            args.extend(s(&["-an", "-c:v", "copy"]));
            ("-muted", e.clone())
        }
        ("rotate-cw", true) => {
            let mut v = video_codec(&e, crf_for(opts.quality));
            if v.is_empty() {
                v = video_codec("mp4", crf_for(opts.quality));
            }
            if let Some(i) = v.iter().position(|a| a == "-vf") {
                v[i + 1] = format!("transpose=1,{EVEN}");
            } else {
                v.extend(s(&["-vf", "transpose=1"]));
            }
            args.extend(v);
            (
                "-rotated",
                if video_codec(&e, 20).is_empty() {
                    "mp4".into()
                } else {
                    e.clone()
                },
            )
        }
        ("speed-2x", true) => {
            let mut v = video_codec("mp4", crf_for(opts.quality));
            let i = v
                .iter()
                .position(|a| a == "-vf")
                .expect("mp4 args have -vf");
            v[i + 1] = format!("setpts=0.5*PTS,{EVEN}");
            args.extend(v);
            args.extend(s(&["-filter:a", "atempo=2.0"]));
            ("-2x", "mp4".into())
        }
        ("thumbnail", true) => {
            let at = total.map(|t| (t * 0.1).min(5.0)).unwrap_or(0.0);
            args = vec![
                "-ss".into(),
                format!("{at:.2}"),
                "-i".into(),
                input.to_string_lossy().into_owned(),
            ];
            args.extend(s(&["-frames:v", "1", "-q:v", "2"]));
            ("-thumbnail", "jpg".into())
        }
        ("normalize", false) => {
            args.extend(s(&["-af", "loudnorm=I=-16:TP=-1.5:LRA=11"]));
            args.extend(audio_codec(&e));
            ("-normalized", e.clone())
        }
        ("mono", false) => {
            args.extend(s(&["-ac", "1"]));
            args.extend(audio_codec(&e));
            ("-mono", e.clone())
        }
        ("trim-silence", false) => {
            args.extend(s(&[
                "-af",
                "silenceremove=start_periods=1:start_threshold=-50dB,areverse,silenceremove=start_periods=1:start_threshold=-50dB,areverse",
            ]));
            args.extend(audio_codec(&e));
            ("-trimmed", e.clone())
        }
        _ => {
            return Err(Error::Unsupported {
                op: format!("tool:{name}"),
                file: input.display().to_string(),
            })
        }
    };
    let out = output_path(input, suffix, &target, opts);
    run_ffmpeg(&args, &out, total, progress)?;
    Ok(out)
}

/// Concatenate files end to end (re-encoded, so mixed sources work).
pub fn concat(inputs: &[PathBuf], opts: &Options, progress: Progress) -> Result<PathBuf> {
    let video = inputs.iter().all(|p| detect(p) == Kind::Video);
    let mut args = Vec::new();
    for p in inputs {
        args.extend(input_args(p));
    }
    let n = inputs.len();
    let total: Option<f64> = inputs.iter().map(|p| duration(p)).sum();
    let target = if video {
        "mp4".to_string()
    } else {
        ext(&inputs[0])
    };
    if video {
        // Scale everything to the first clip's size so the concat filter accepts it.
        let mut graph = String::new();
        for i in 0..n {
            graph.push_str(&format!(
                "[{i}:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30[v{i}];[{i}:a]aresample=48000[a{i}];"
            ));
        }
        for i in 0..n {
            graph.push_str(&format!("[v{i}][a{i}]"));
        }
        graph.push_str(&format!("concat=n={n}:v=1:a=1[v][a]"));
        args.extend([
            "-filter_complex".into(),
            graph,
            "-map".into(),
            "[v]".into(),
            "-map".into(),
            "[a]".into(),
        ]);
        args.extend(s(&[
            "-c:v",
            "libx264",
            "-crf",
            "20",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-movflags",
            "+faststart",
        ]));
    } else {
        let mut graph = String::new();
        for i in 0..n {
            graph.push_str(&format!("[{i}:a]"));
        }
        graph.push_str(&format!("concat=n={n}:v=0:a=1[a]"));
        args.extend(["-filter_complex".into(), graph, "-map".into(), "[a]".into()]);
        args.extend(audio_codec(&target));
    }
    let out = output_path(&inputs[0], "-merged", &target, opts);
    run_ffmpeg(&args, &out, total, progress)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_timestamp("00:01:02.50"), Some(62.5));
        assert_eq!(parse_timestamp("1:00:00.00"), Some(3600.0));
    }

    #[test]
    fn crf_mapping_is_sane() {
        assert_eq!(crf_for(85), 20);
        assert!(crf_for(1) > crf_for(100));
    }

    #[test]
    fn converts_generated_media() {
        let Ok(ff) = ffmpeg() else { return };
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("tone.wav");
        let mp4 = dir.path().join("clip.mkv");
        assert!(Command::new(&ff)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:d=1"
            ])
            .arg(&wav)
            .status()
            .unwrap()
            .success());
        assert!(Command::new(&ff)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=161x91:d=1",
                "-f",
                "lavfi",
                "-i",
                "sine=d=1",
                "-shortest",
                "-c:v",
                "ffv1",
                "-c:a",
                "flac"
            ])
            .arg(&mp4)
            .status()
            .unwrap()
            .success());
        let opts = Options::default();
        for t in ["mp3", "m4a", "flac", "ogg", "opus", "aiff"] {
            assert!(convert(&wav, t, &opts, &|_| {}).unwrap().exists(), "{t}");
        }
        for t in ["mp4", "mov", "webm", "avi", "wmv", "gif", "mp3"] {
            assert!(convert(&mp4, t, &opts, &|_| {})
                .unwrap_or_else(|e| panic!("{t}: {e}"))
                .exists());
        }
        for t in [
            "compress",
            "thumbnail",
            "mute",
            "rotate-cw",
            "speed-2x",
            "extract-audio",
        ] {
            assert!(tool(&mp4, t, &opts, &|_| {})
                .unwrap_or_else(|e| panic!("{t}: {e}"))
                .exists());
        }
        assert!(concat(&[mp4.clone(), mp4.clone()], &opts, &|_| {})
            .unwrap()
            .exists());
        assert!(concat(&[wav.clone(), wav.clone()], &opts, &|_| {})
            .unwrap()
            .exists());
    }
}
