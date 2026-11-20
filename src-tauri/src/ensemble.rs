//! Reads a frame several ways (different image clean-ups) and keeps the best reading of every line.
//!
//! Which clean-up works depends on the game: pixel fonts want the raw image, faint text wants a
//! contrast stretch, outlined subtitles want only their bright fill. Rather than make the user pick,
//! the variants compete and `quality` judges them.
//!
//! Upscaling helps OCR a lot, but doing it to the whole frame is slow and mostly wasted on background.
//! So this works in two steps: find where the text is on the cheap 1x image, then upscale and re-read
//! only those regions. Cost follows the amount of text, not the size or busyness of the frame.

use crate::{
    ocr::{self, Line},
    preprocess::{self as pp, Frame},
    quality::{choose, similarity, Candidate, Quality},
};
use std::thread;

/// Text line height OCR reads best at; smaller regions are upscaled towards it.
const TARGET_LINE_HEIGHT: f32 = 56.0;
const MAX_FACTOR: f32 = 3.0;
/// Even large-looking text reads better upscaled a little (thin strokes, busy backgrounds). Measured on
/// a small set of hard images: 1.0 and 2.0 both misread a stylised font that 1.6 reads at 96%.
const MIN_FACTOR: f32 = 1.6;
/// Upscaled pixels per region pass.
const MAX_REGION_PIXELS: f32 = 600_000.0;
/// Most text regions refined per frame, best first.
const MAX_REGIONS: usize = 12;

type Prep = fn(&Frame, f32) -> Frame;

fn radius(k: f32) -> u32 {
    (9.0 * k).round() as u32
}

fn gray(f: &Frame, k: f32) -> Frame {
    pp::gray_levels(&pp::upscale(f, k))
}
fn plain(f: &Frame, k: f32) -> Frame {
    pp::upscale(f, k)
}
fn bright(f: &Frame, k: f32) -> Frame {
    pp::bright_mask(&pp::upscale(f, k), 200)
}
fn otsu(f: &Frame, k: f32) -> Frame {
    pp::otsu(&pp::upscale(f, k))
}
fn local(f: &Frame, k: f32) -> Frame {
    pp::local_contrast(&pp::upscale(f, k), radius(k))
}
fn adapt_dark(f: &Frame, k: f32) -> Frame {
    pp::adaptive(&pp::upscale(f, k), radius(k), 18, false)
}
fn adapt_light(f: &Frame, k: f32) -> Frame {
    pp::adaptive(&pp::upscale(f, k), radius(k), 18, true)
}

const ALL: &[Prep] = &[gray, plain, bright, otsu, local, adapt_dark, adapt_light];
/// Enough to notice that text is present at all.
const QUICK: &[Prep] = &[gray, plain];

struct Reading {
    lines: Vec<Line>,
}

/// One OCR pass per variant, in parallel (each thread has its own engine). Line positions are
/// reported in the pixels of the frame that was passed in, not the upscaled copy.
fn run(tag: &str, frame: &Frame, factor: f32, preps: &[Prep]) -> Vec<Reading> {
    thread::scope(|scope| {
        let handles: Vec<_> = preps
            .iter()
            .map(|prep| {
                scope.spawn(move || {
                    let mut lines = ocr::create_engine(tag)
                        .and_then(|engine| {
                            let img = prep(frame, factor);
                            ocr::recognize(&engine, &img.bgra, img.width, img.height)
                        })
                        .unwrap_or_default();
                    lines.iter_mut().for_each(|l| {
                        l.x /= factor;
                        l.y /= factor;
                        l.w /= factor;
                        l.h /= factor;
                    });
                    Reading { lines }
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

/// How much of the smaller box the two boxes share (0..1).
fn overlap(a: &Line, b: &Line) -> f32 {
    let ix = ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.0);
    let iy = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.0);
    let smaller = (a.w * a.h).min(b.w * b.h);
    if smaller <= 0.0 {
        0.0
    } else {
        ix * iy / smaller
    }
}

/// A place on the frame where at least one variant saw text, with what each variant read there.
struct Region {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    line_height: f32,
    seen: Vec<String>,
}

fn locate(found: &[Reading]) -> Vec<Region> {
    let mut clusters: Vec<Vec<&Line>> = Vec::new();
    for line in found.iter().flat_map(|r| r.lines.iter()) {
        match clusters.iter_mut().find(|c| c.iter().any(|m| overlap(m, line) >= 0.5)) {
            Some(c) => c.push(line),
            None => clusters.push(vec![line]),
        }
    }

    let mut scored: Vec<(i32, Region)> = clusters
        .into_iter()
        .filter_map(|c| {
            let seen: Vec<String> = c.iter().map(|l| l.text.clone()).collect();
            let cands: Vec<Candidate> = seen.iter().cloned().map(Candidate::new).collect();
            let q = cands[choose(&cands)].q;
            // Loose gate: something word-like. The strict check happens after refinement.
            if q.good < 2 {
                return None;
            }
            let x0 = c.iter().map(|l| l.x).fold(f32::MAX, f32::min);
            let y0 = c.iter().map(|l| l.y).fold(f32::MAX, f32::min);
            let x1 = c.iter().map(|l| l.x + l.w).fold(0.0, f32::max);
            let y1 = c.iter().map(|l| l.y + l.h).fold(0.0, f32::max);
            let mut heights: Vec<f32> = c.iter().map(|l| l.h).collect();
            heights.sort_by(f32::total_cmp);
            Some((q.score(), Region { x: x0, y: y0, w: x1 - x0, h: y1 - y0, line_height: heights[heights.len() / 2], seen }))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(MAX_REGIONS).map(|(_, r)| r).collect()
}

struct Refined {
    line: Line,
    quality: Quality,
    trusted: bool,
}

/// Upscale just this region and let every variant read it; keep the most trustworthy reading.
fn refine(tag: &str, frame: &Frame, r: &Region) -> Option<Refined> {
    let pad = (0.35 * r.line_height).max(4.0);
    let x0 = (r.x - pad).max(0.0) as u32;
    let y0 = (r.y - pad).max(0.0) as u32;
    let x1 = ((r.x + r.w + pad).ceil() as u32).min(frame.width);
    let y1 = ((r.y + r.h + pad).ceil() as u32).min(frame.height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let crop = pp::crop(frame, x0, y0, x1 - x0, y1 - y0);

    let by_height = (TARGET_LINE_HEIGHT / r.line_height.max(1.0)).clamp(MIN_FACTOR, MAX_FACTOR);
    let by_budget = (MAX_REGION_PIXELS / (crop.width as f32 * crop.height as f32)).sqrt();
    let factor = by_height.min(by_budget).max(1.0);

    // The crop can include slivers of neighbouring lines; keep only lines centred in our band.
    let (keep_top, keep_bottom) = (r.y - 0.3 * r.line_height, r.y + r.h + 0.3 * r.line_height);
    let readings: Vec<Vec<Line>> = run(tag, &crop, factor, ALL)
        .into_iter()
        .map(|reading| {
            reading
                .lines
                .into_iter()
                .map(|mut l| {
                    l.x += x0 as f32;
                    l.y += y0 as f32;
                    l
                })
                .filter(|l| {
                    let cy = l.y + l.h / 2.0;
                    cy >= keep_top && cy <= keep_bottom
                })
                .collect()
        })
        .collect();

    let mut cands: Vec<Candidate> = readings
        .iter()
        .map(|lines| Candidate::new(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ")))
        .collect();
    cands.extend(r.seen.iter().cloned().map(Candidate::new));

    let best = choose(&cands);
    let supporters = cands
        .iter()
        .enumerate()
        .filter(|(i, c)| *i != best && similarity(&c.text, &cands[best].text) >= 0.8)
        .count();
    let q = cands[best].q;
    // Long, clean text stands on its own; short text needs a second variant to agree.
    let trusted = q.ratio() >= 0.85 && (q.good >= 4 || (q.good >= 2 && supporters >= 1));

    // Box the text where the winning variant actually found it; fall back to where it was first seen.
    let (x, y, w, h) = match readings.get(best).filter(|l| !l.is_empty()) {
        Some(lines) => {
            let x0 = lines.iter().map(|l| l.x).fold(f32::MAX, f32::min);
            let y0 = lines.iter().map(|l| l.y).fold(f32::MAX, f32::min);
            let x1 = lines.iter().map(|l| l.x + l.w).fold(0.0, f32::max);
            let y1 = lines.iter().map(|l| l.y + l.h).fold(0.0, f32::max);
            (x0, y0, x1 - x0, y1 - y0)
        }
        None => (r.x, r.y, r.w, r.h),
    };
    Some(Refined { line: Line { text: cands[best].text.clone(), x, y, w, h }, quality: q, trusted })
}

/// Different regions can resolve to the same text once their crops are widened; keep the better one.
fn dedupe(mut lines: Vec<(Line, i32)>) -> Vec<Line> {
    lines.sort_by(|a, b| b.1.cmp(&a.1));
    let mut kept: Vec<(Line, i32)> = Vec::new();
    for (line, score) in lines {
        if !kept.iter().any(|(k, _)| overlap(k, &line) >= 0.5) {
            kept.push((line, score));
        }
    }
    kept.into_iter().map(|(l, _)| l).collect()
}

/// `full = false` only looks for text with a cheap pair of variants; the caller uses that to stay
/// light while a scene has none. Anything found is still refined with every variant.
pub fn read(tag: &str, frame: &Frame, full: bool) -> Vec<Line> {
    let found = run(tag, frame, 1.0, if full { ALL } else { QUICK });
    let regions = locate(&found);
    // Regions are independent, so refine them side by side.
    let refined: Vec<Refined> = thread::scope(|scope| {
        let handles: Vec<_> = regions.iter().map(|r| scope.spawn(move || refine(tag, frame, r))).collect();
        handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });

    let mut kept: Vec<Line> = Vec::new();
    if refined.iter().any(|r| r.trusted) {
        kept = dedupe(refined.into_iter().filter(|r| r.trusted).map(|r| (r.line, r.quality.score())).collect());
    } else if let Some(best) = refined.into_iter().max_by_key(|r| r.quality.score()) {
        // Nothing passed the strict check (stylised fonts often read imperfectly). Better one imperfect
        // line than none, but only if it is substantial: noise rarely yields six word-like characters.
        if best.quality.good >= 6 && best.quality.ratio() >= 0.5 {
            kept.push(best.line);
        }
    }
    kept.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    kept
}


