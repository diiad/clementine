//! SRT and WebVTT subtitles.

use crate::kind::ext;
use crate::{output_path, Error, Options, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    /// Milliseconds.
    pub start: i64,
    pub end: i64,
    pub text: String,
}

fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim().replace(',', ".");
    let (hms, ms) = s.split_once('.').unwrap_or((&s, "0"));
    let parts: Vec<i64> = hms
        .split(':')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    let (h, m, sec) = match parts[..] {
        [h, m, s] => (h, m, s),
        [m, s] => (0, m, s),
        _ => return None,
    };
    let ms: i64 = format!("{:0<3}", &ms[..ms.len().min(3)]).parse().ok()?;
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

fn fmt_time(ms: i64, sep: char) -> String {
    let ms = ms.max(0);
    format!(
        "{:02}:{:02}:{:02}{sep}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Parses both SRT and VTT: blocks separated by blank lines, each with a `-->` line.
pub fn parse(src: &str) -> Vec<Cue> {
    let src = src.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in src.split("\n\n") {
        let mut lines = block.lines().skip_while(|l| !l.contains("-->"));
        let Some(timing) = lines.next() else { continue };
        let Some((a, b)) = timing.split_once("-->") else {
            continue;
        };
        // VTT allows cue settings after the end time.
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_time(a), parse_time(b)) else {
            continue;
        };
        let text = lines.collect::<Vec<_>>().join("\n");
        cues.push(Cue { start, end, text });
    }
    cues
}

pub fn to_srt(cues: &[Cue]) -> String {
    cues.iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{}\n{} --> {}\n{}\n",
                i + 1,
                fmt_time(c.start, ','),
                fmt_time(c.end, ','),
                c.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn to_vtt(cues: &[Cue]) -> String {
    let mut s = String::from("WEBVTT\n\n");
    for c in cues {
        s.push_str(&format!(
            "{} --> {}\n{}\n\n",
            fmt_time(c.start, '.'),
            fmt_time(c.end, '.'),
            c.text
        ));
    }
    s
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn write(cues: &[Cue], target: &str, out: &Path) -> Result<()> {
    let body = match target {
        "srt" => to_srt(cues),
        "vtt" => to_vtt(cues),
        "txt" => {
            cues.iter()
                .map(|c| strip_tags(&c.text))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        }
        other => return Err(anyhow::anyhow!("cannot write {other} subtitles").into()),
    };
    std::fs::write(out, body)?;
    Ok(())
}

fn load(input: &Path) -> Result<Vec<Cue>> {
    let bytes = std::fs::read(input)?;
    Ok(parse(&String::from_utf8_lossy(&bytes)))
}

pub fn convert(input: &Path, target: &str, opts: &Options) -> Result<PathBuf> {
    let cues = load(input)?;
    let out = output_path(input, "", target, opts);
    write(&cues, target, &out)?;
    Ok(out)
}

pub fn tool(input: &Path, name: &str, opts: &Options) -> Result<PathBuf> {
    let delta = match name {
        "shift+1" => 1000,
        "shift-1" => -1000,
        _ => {
            return Err(Error::Unsupported {
                op: format!("tool:{name}"),
                file: input.display().to_string(),
            })
        }
    };
    let cues: Vec<Cue> = load(input)?
        .into_iter()
        .map(|c| Cue {
            start: (c.start + delta).max(0),
            end: (c.end + delta).max(0),
            ..c
        })
        .collect();
    let target = ext(input);
    let out = output_path(
        input,
        if delta > 0 { "-delayed" } else { "-advanced" },
        &target,
        opts,
    );
    write(&cues, &target, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\r\n00:00:01,500 --> 00:00:03,000\r\nHello <i>world</i>\r\n\r\n2\r\n00:01:00,000 --> 00:01:02,250\r\nSecond\r\nline\r\n";

    #[test]
    fn srt_vtt_round_trip() {
        let cues = parse(SRT);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, 1500);
        assert_eq!(cues[1].text, "Second\nline");
        let vtt = to_vtt(&cues);
        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("00:00:01.500 --> 00:00:03.000"));
        assert_eq!(parse(&vtt), cues);
        assert_eq!(parse(&to_srt(&cues)), cues);
    }

    #[test]
    fn vtt_short_times_and_settings() {
        let cues = parse("WEBVTT\n\n01:02.5 --> 01:04.000 align:start\nHi\n");
        assert_eq!(cues[0].start, 62_500);
        assert_eq!(cues[0].end, 64_000);
    }
}
