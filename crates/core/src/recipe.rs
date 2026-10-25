//! Recipes: several operations chained into one, e.g. "Fit 1080 px, then
//! JPG". Each step works on the files the previous one produced; the steps
//! in between write to a scratch folder and only the last one's results are
//! kept.

use crate::engines::scratch;
use crate::kind::ext;
use crate::registry::same_format;
use crate::{operations, run, unique_path, Category, Error, Options, Progress, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    pub name: String,
    /// Operation ids, run in order. An action (`action:copy`) may only come last.
    pub steps: Vec<String>,
}

impl Recipe {
    /// Whether the recipe can start on `inputs`: its first real step is
    /// offered for them (a leading conversion to their own format is skipped).
    pub fn applies(&self, inputs: &[PathBuf]) -> bool {
        if inputs.is_empty() || self.steps.is_empty() {
            return false;
        }
        if combines(&self.steps) {
            return first_step_offered(&self.steps, inputs);
        }
        inputs
            .iter()
            .all(|p| first_step_offered(&self.steps, std::slice::from_ref(p)))
    }
}

fn first_step_offered(steps: &[String], inputs: &[PathBuf]) -> bool {
    let Some(step) = steps.iter().find(|s| !inputs.iter().all(|p| skips(s, p))) else {
        return true; // every step is a no-op: harmless
    };
    let category = match step.split_once(':').map(|(c, _)| c) {
        Some("convert") => Category::Convert,
        Some("tool") => Category::Tool,
        Some("action") => Category::Action,
        _ => return false,
    };
    operations(inputs, category).iter().any(|o| &o.id == step)
}

/// A conversion to the format the file is already in does nothing.
fn skips(step: &str, input: &Path) -> bool {
    step.strip_prefix("convert:")
        .is_some_and(|t| same_format(t, &ext(input)))
}

/// Steps that turn several files into one.
fn combines(steps: &[String]) -> bool {
    steps
        .iter()
        .any(|s| matches!(s.as_str(), "tool:merge" | "tool:merge-pdf" | "tool:archive"))
}

/// Run `steps` on `inputs` and return the files produced by the last step.
///
/// Without combining steps each file goes through the recipe on its own, so
/// its results land next to it (or in `opts.out_dir`).
pub fn run_recipe(
    steps: &[String],
    inputs: &[PathBuf],
    opts: &Options,
    progress: Progress,
) -> Result<Vec<PathBuf>> {
    if steps.is_empty() {
        return Err(Error::UnknownOperation("empty recipe".into()));
    }
    if let Some(i) = steps.iter().position(|s| s.starts_with("action:")) {
        if i + 1 != steps.len() {
            return Err(Error::UnknownOperation(format!(
                "{}: an action can only be the last step",
                steps[i]
            )));
        }
    }
    if combines(steps) {
        return chain(steps, inputs, opts, progress);
    }
    let n = inputs.len() as f32;
    let mut out = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        let sub = |f: f32| progress((i as f32 + f) / n);
        out.extend(chain(steps, std::slice::from_ref(input), opts, &sub)?);
    }
    progress(1.0);
    Ok(out)
}

fn chain(
    steps: &[String],
    inputs: &[PathBuf],
    opts: &Options,
    progress: Progress,
) -> Result<Vec<PathBuf>> {
    let final_dir = opts
        .out_dir
        .clone()
        .or_else(|| {
            inputs
                .first()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| PathBuf::from("."));
    let tmp = scratch()?;
    let n = steps.len();
    let mut current = inputs.to_vec();
    for (i, step) in steps.iter().enumerate() {
        let last = i + 1 == n;
        let sub = |f: f32| progress((i as f32 + f) / n as f32);
        if step.starts_with("action:") {
            run(step, &current, opts, &sub)?;
            break;
        }
        let dir = if last {
            final_dir.clone()
        } else {
            let d = tmp.path().join(i.to_string());
            std::fs::create_dir_all(&d)?;
            d
        };
        let step_opts = Options {
            out_dir: Some(dir.clone()),
            ..opts.clone()
        };
        let (kept, todo): (Vec<PathBuf>, Vec<PathBuf>) =
            current.into_iter().partition(|p| skips(step, p));
        let mut next = Vec::new();
        for p in kept {
            // Already in that format: pass it on, out of the scratch folder
            // if this is the end.
            if last && p.starts_with(tmp.path()) {
                let dest = unique_path(&dir.join(p.file_name().unwrap_or_default()));
                std::fs::copy(&p, &dest)?;
                next.push(dest);
            } else {
                next.push(p);
            }
        }
        if !todo.is_empty() {
            next.extend(run(step, &todo, &step_opts, &sub)?);
        }
        current = next;
    }
    progress(1.0);
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        image::RgbaImage::from_pixel(1600, 800, image::Rgba([200, 100, 50, 255]))
            .save(&p)
            .unwrap();
        p
    }

    fn steps(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn chains_steps_and_keeps_only_the_result() {
        let dir = tempfile::tempdir().unwrap();
        let src = png(dir.path(), "photo.png");
        let out = run_recipe(
            &steps(&["tool:resize-1080", "convert:jpg"]),
            &[src],
            &Options::default(),
            &|_| {},
        )
        .unwrap();
        assert_eq!(out, vec![dir.path().join("photo-1080.jpg")]);
        let img = image::open(&out[0]).unwrap();
        assert_eq!((img.width(), img.height()), (1080, 540));
        // Nothing from the intermediate step is left next to the original.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn skips_conversion_to_the_same_format() {
        let dir = tempfile::tempdir().unwrap();
        let src = png(dir.path(), "a.png");
        let recipe = Recipe {
            name: "PNG".into(),
            steps: steps(&["convert:png", "tool:grayscale"]),
        };
        assert!(recipe.applies(std::slice::from_ref(&src)));
        let out = run_recipe(&recipe.steps, &[src], &Options::default(), &|_| {}).unwrap();
        assert_eq!(out, vec![dir.path().join("a-gray.png")]);
    }

    #[test]
    fn applies_only_where_the_first_step_does() {
        let recipe = Recipe {
            name: "Web".into(),
            steps: steps(&["tool:resize-1080", "convert:jpg"]),
        };
        assert!(recipe.applies(&["a.png".into(), "b.heic".into()]));
        assert!(!recipe.applies(&["a.png".into(), "song.mp3".into()]));
        assert!(!recipe.applies(&[]));
    }

    #[test]
    fn actions_must_come_last() {
        let err = run_recipe(
            &steps(&["action:copy", "convert:jpg"]),
            &["a.png".into()],
            &Options::default(),
            &|_| {},
        );
        assert!(err.is_err());
    }
}
