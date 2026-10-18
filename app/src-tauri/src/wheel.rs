//! Wheel geometry, shared by the renderer (sent to the web view) and the
//! drop hit-test (done here, in Rust, from the native drag-and-drop events).

use satsuma_core::Operation;
use serde::Serialize;

/// Window edge, in logical pixels.
pub const SIZE: f64 = 420.0;
const CENTER: f64 = SIZE / 2.0;
/// Inside this radius is the cancel / status hub.
pub const HUB: f64 = 58.0;
const OUTER: f64 = 200.0;
const SPLIT: f64 = 124.0;
/// More operations than this use two rings.
const ONE_RING_MAX: usize = 12;

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub id: String,
    pub label: String,
    /// Degrees, clockwise from 12 o'clock. `start` may be negative.
    pub start: f64,
    pub end: f64,
    pub r_in: f64,
    pub r_out: f64,
}

fn ring(ops: &[Operation], r_in: f64, r_out: f64, out: &mut Vec<Segment>) {
    let n = ops.len();
    if n == 0 {
        return;
    }
    let step = 360.0 / n as f64;
    for (i, op) in ops.iter().enumerate() {
        let mid = i as f64 * step;
        out.push(Segment {
            id: op.id.clone(),
            label: op.label.clone(),
            start: mid - step / 2.0,
            end: mid + step / 2.0,
            r_in,
            r_out,
        });
    }
}

pub fn layout(ops: &[Operation]) -> Vec<Segment> {
    let mut segs = Vec::with_capacity(ops.len());
    if ops.len() <= ONE_RING_MAX {
        ring(ops, HUB, OUTER, &mut segs);
    } else {
        // The inner ring is shorter, so it gets fewer slices.
        let inner = (ops.len() as f64 * 0.4).round() as usize;
        ring(&ops[..inner], HUB, SPLIT, &mut segs);
        ring(&ops[inner..], SPLIT, OUTER, &mut segs);
    }
    segs
}

#[derive(Debug, PartialEq, Eq)]
pub enum Hit {
    Segment(usize),
    Hub,
    Outside,
}

/// Which part of the wheel is under `(x, y)` (logical, window-relative).
pub fn hit(segs: &[Segment], x: f64, y: f64) -> Hit {
    let (dx, dy) = (x - CENTER, y - CENTER);
    let r = dx.hypot(dy);
    if r < HUB {
        return Hit::Hub;
    }
    let angle = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
    for (i, s) in segs.iter().enumerate() {
        if r < s.r_in || r >= s.r_out {
            continue;
        }
        let within = |a: f64| a >= s.start && a < s.end;
        if within(angle) || within(angle - 360.0) {
            return Hit::Segment(i);
        }
    }
    Hit::Outside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(n: usize) -> Vec<Operation> {
        let json: Vec<serde_json::Value> = (0..n)
            .map(|i| serde_json::json!({"id": format!("convert:{i}"), "label": i.to_string(), "category": "convert"}))
            .collect();
        json.into_iter()
            .map(|v| serde_json::from_value(v).unwrap())
            .collect()
    }

    #[test]
    fn hit_test_matches_layout() {
        let segs = layout(&ops(4));
        // Straight up is segment 0, right is 1, down is 2, left is 3.
        assert_eq!(hit(&segs, CENTER, CENTER - 150.0), Hit::Segment(0));
        assert_eq!(hit(&segs, CENTER + 150.0, CENTER), Hit::Segment(1));
        assert_eq!(hit(&segs, CENTER, CENTER + 150.0), Hit::Segment(2));
        assert_eq!(hit(&segs, CENTER - 150.0, CENTER), Hit::Segment(3));
        // Segment 0 spans -45°..45°, so slightly left of top is still 0.
        assert_eq!(hit(&segs, CENTER - 20.0, CENTER - 150.0), Hit::Segment(0));
        assert_eq!(hit(&segs, CENTER, CENTER), Hit::Hub);
        assert_eq!(hit(&segs, 1.0, 1.0), Hit::Outside);
    }

    #[test]
    fn many_ops_use_two_rings() {
        let segs = layout(&ops(15));
        assert_eq!(segs.len(), 15);
        assert_eq!(segs.iter().filter(|s| s.r_out == SPLIT).count(), 6);
        assert!(matches!(hit(&segs, CENTER, CENTER - 90.0), Hit::Segment(0)));
        assert!(matches!(
            hit(&segs, CENTER, CENTER - 160.0),
            Hit::Segment(6)
        ));
    }
}
