//! Scores OCR preprocessing variants and selection strategies against ground truth.
//! Run: OCR_SET=<dir with PNGs and manifest.json> cargo test --lib ocr_bench -- --ignored --nocapture

use crate::{
    ocr,
    preprocess::{adaptive, bright_mask, gray_levels, local_contrast, otsu, upscale, Frame},
    quality::{choose, confident, similarity, Candidate},
};
use std::path::PathBuf;

type Variant = (&'static str, Box<dyn Fn(&Frame) -> Frame>);

/// Ordered roughly by how often each variant is the right one.
fn variants() -> Vec<Variant> {
    vec![
        ("up2+gray", Box::new(|f| gray_levels(&upscale(f, 2.0)))),
        ("up2", Box::new(|f| upscale(f, 2.0))),
        ("up2+bright200", Box::new(|f| bright_mask(&upscale(f, 2.0), 200))),
        ("up2+otsu", Box::new(|f| otsu(&upscale(f, 2.0)))),
        ("up2+local", Box::new(|f| local_contrast(&upscale(f, 2.0), 18))),
        ("up2+adaptDark", Box::new(|f| adaptive(&upscale(f, 2.0), 18, 18, false))),
        ("up2+adaptLight", Box::new(|f| adaptive(&upscale(f, 2.0), 18, 18, true))),
        ("raw", Box::new(|f| f.clone())),
    ]
}

#[test]
#[ignore]
fn bench() {
    let dir = PathBuf::from(std::env::var("OCR_SET").expect("set OCR_SET to the test image folder"));
    let raw = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap();
    let engine = ocr::create_engine("en-US").expect("OCR engine");
    let variants = variants();
    let items: Vec<&serde_json::Value> = manifest.as_array().unwrap().iter().filter(|i| std::env::var("OCR_ONLY").map_or(true, |o| i["file"].as_str().unwrap().contains(&o))).collect();
    let n = variants.len();

    print!("{:<15}", "image");
    variants.iter().for_each(|(name, _)| print!("{name:>15}"));
    println!("{:>10}{:>10}{:>10}{:>8}", "oracle", "chooseAll", "staged", "passes");

    let (mut tot, mut oracle_tot, mut all_tot, mut staged_tot, mut passes_tot) = (vec![0f32; n], 0f32, 0f32, 0f32, 0usize);
    let mut notes = Vec::new();

    for item in &items {
        let file = item["file"].as_str().unwrap();
        let truth = item["truth"].as_str().unwrap();
        let img = image::open(dir.join(file)).unwrap().to_rgba8();
        let (w, h) = img.dimensions();
        let mut bgra = img.into_raw();
        bgra.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
        let frame = Frame::new(w, h, bgra);

        let cands: Vec<Candidate> = variants
            .iter()
            .map(|(_, f)| {
                let out = f(&frame);
                let lines = ocr::recognize(&engine, &out.bgra, out.width, out.height).unwrap_or_default();
                Candidate::new(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" "))
            })
            .collect();
        let sims: Vec<f32> = cands.iter().map(|c| similarity(&c.text, truth)).collect();

        // Everything, then pick by plausibility + agreement.
        let pick_all = choose(&cands);
        // In order, stopping as soon as two variants agree on clean text.
        let mut tried = 0;
        let mut staged = None;
        for i in 0..n {
            tried = i + 1;
            if let Some(k) = confident(&cands[..tried]) {
                staged = Some(k);
                break;
            }
        }
        let staged = staged.unwrap_or_else(|| choose(&cands));
        let oracle = sims.iter().cloned().fold(0.0, f32::max);

        print!("{:<15}", file.trim_end_matches(".png"));
        for (i, s) in sims.iter().enumerate() {
            print!("{:>14.0}%", s * 100.0);
            tot[i] += s;
        }
        println!("{:>9.0}%{:>9.0}%{:>9.0}%{:>8}", oracle * 100.0, sims[pick_all] * 100.0, sims[staged] * 100.0, tried);
        oracle_tot += oracle;
        all_tot += sims[pick_all];
        staged_tot += sims[staged];
        passes_tot += tried;
        notes.push(format!(
            "{:<14} staged picked {:<14} -> {:?}",
            file.trim_end_matches(".png"), variants[staged].0, cands[staged].text
        ));
    }

    let m = items.len() as f32;
    print!("{:<15}", "AVERAGE");
    tot.iter().for_each(|t| print!("{:>14.0}%", t / m * 100.0));
    println!(
        "{:>9.0}%{:>9.0}%{:>9.0}%{:>8.1}",
        oracle_tot / m * 100.0, all_tot / m * 100.0, staged_tot / m * 100.0, passes_tot as f32 / m
    );
    println!("--- what the staged strategy chose:");
    notes.iter().for_each(|l| println!("{l}"));
}

/// The real pipeline end to end: accuracy and time per frame, against plain single-pass OCR.
#[test]
#[ignore]
fn pipeline() {
    use std::time::Instant;
    let dir = PathBuf::from(std::env::var("OCR_SET").expect("set OCR_SET to the test image folder"));
    let raw = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap();
    let engine = ocr::create_engine("en-US").expect("OCR engine");
    let items: Vec<&serde_json::Value> = manifest.as_array().unwrap().iter().filter(|i| std::env::var("OCR_ONLY").map_or(true, |o| i["file"].as_str().unwrap().contains(&o))).collect();

    println!("{:<15}{:>10}{:>12}{:>10}{:>10}", "image", "plain", "enhanced", "plain ms", "enh ms");
    let (mut plain_tot, mut enh_tot, mut plain_ms, mut enh_ms, mut worst_ms) = (0f32, 0f32, 0u128, 0u128, 0u128);
    for item in &items {
        let file = item["file"].as_str().unwrap();
        let truth = item["truth"].as_str().unwrap();
        let img = image::open(dir.join(file)).unwrap().to_rgba8();
        let (w, h) = img.dimensions();
        let mut bgra = img.into_raw();
        bgra.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
        let frame = Frame::new(w, h, bgra);
        let join = |lines: &[ocr::Line]| lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");

        let t = Instant::now();
        let plain = ocr::recognize(&engine, &frame.bgra, w, h).unwrap_or_default();
        let (p_ms, p_sim) = (t.elapsed().as_millis(), similarity(&join(&plain), truth));
        let t = Instant::now();
        let enhanced = crate::ensemble::read("en-US", &frame, true);
        let (e_ms, e_sim) = (t.elapsed().as_millis(), similarity(&join(&enhanced), truth));

        println!("{:<15}{:>9.0}%{:>11.0}%{:>10}{:>10}", file.trim_end_matches(".png"), p_sim * 100.0, e_sim * 100.0, p_ms, e_ms);
        plain_tot += p_sim;
        enh_tot += e_sim;
        plain_ms += p_ms;
        enh_ms += e_ms;
        worst_ms = worst_ms.max(e_ms);
    }
    let m = items.len() as f32;
    println!(
        "{:<15}{:>9.0}%{:>11.0}%{:>10}{:>10}   (slowest enhanced frame: {} ms)",
        "AVERAGE", plain_tot / m * 100.0, enh_tot / m * 100.0, plain_ms / items.len() as u128, enh_ms / items.len() as u128, worst_ms
    );
}

