//! Image clean-up applied before OCR. Windows OCR is trained on document-like text, so game text
//! (small, outlined, stylised, over busy scenes) reads far better after upscaling and simplifying.

use image::{imageops::{resize, FilterType}, ImageBuffer, Rgba};

/// A BGRA frame, 4 bytes per pixel, rows tightly packed.
#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32, bgra: Vec<u8>) -> Self {
        debug_assert_eq!(bgra.len(), width as usize * height as usize * 4);
        Self { width, height, bgra }
    }

    fn luma(&self) -> Vec<u8> {
        self.bgra
            .chunks_exact(4)
            .map(|p| ((29 * p[0] as u32 + 150 * p[1] as u32 + 77 * p[2] as u32) >> 8) as u8)
            .collect()
    }

    fn from_luma(width: u32, height: u32, luma: &[u8]) -> Self {
        let mut bgra = Vec::with_capacity(luma.len() * 4);
        for &v in luma {
            bgra.extend_from_slice(&[v, v, v, 255]);
        }
        Self { width, height, bgra }
    }
}

/// The part of `f` inside the given rectangle (already clipped to the frame).
pub fn crop(f: &Frame, x: u32, y: u32, w: u32, h: u32) -> Frame {
    let mut bgra = Vec::with_capacity(w as usize * h as usize * 4);
    for row in y..y + h {
        let start = (row as usize * f.width as usize + x as usize) * 4;
        bgra.extend_from_slice(&f.bgra[start..start + w as usize * 4]);
    }
    Frame { width: w, height: h, bgra }
}

/// Smooth upscale. Small text gains stroke width, which is what the OCR engine needs.
pub fn upscale(f: &Frame, factor: f32) -> Frame {
    if factor <= 1.05 {
        return f.clone();
    }
    // Channel order does not matter for resizing, so BGRA can be treated as RGBA.
    let src = ImageBuffer::<Rgba<u8>, &[u8]>::from_raw(f.width, f.height, f.bgra.as_slice()).expect("frame size");
    let out = resize(&src, (f.width as f32 * factor) as u32, (f.height as f32 * factor) as u32, FilterType::CatmullRom);
    Frame { width: out.width(), height: out.height(), bgra: out.into_raw() }
}

/// Grayscale with the 1st..99th brightness percentiles stretched to the full range (fixes low contrast).
pub fn gray_levels(f: &Frame) -> Frame {
    let luma = f.luma();
    let mut hist = [0u32; 256];
    luma.iter().for_each(|&v| hist[v as usize] += 1);
    let total = luma.len() as u32;
    let (mut lo, mut hi, mut acc) = (0usize, 255usize, 0u32);
    for (v, &n) in hist.iter().enumerate() {
        acc += n;
        if acc * 100 >= total {
            lo = v;
            break;
        }
    }
    acc = 0;
    for (v, &n) in hist.iter().enumerate().rev() {
        acc += n;
        if acc * 100 >= total {
            hi = v;
            break;
        }
    }
    if hi <= lo + 8 {
        return Frame::from_luma(f.width, f.height, &luma);
    }
    let span = (hi - lo) as i32;
    let stretched: Vec<u8> = luma
        .iter()
        .map(|&v| (((v as i32 - lo as i32) * 255) / span).clamp(0, 255) as u8)
        .collect();
    Frame::from_luma(f.width, f.height, &stretched)
}

/// Stretches contrast separately around every pixel (deviation from the local average, scaled by the
/// local spread). Unlike `gray_levels`, faint text survives next to bright or dark parts of the frame.
pub fn local_contrast(f: &Frame, radius: u32) -> Frame {
    let luma = f.luma();
    let (w, h) = (f.width as usize, f.height as usize);
    let stride = w + 1;
    let mut sum = vec![0u64; stride * (h + 1)];
    let mut sq = vec![0u64; stride * (h + 1)];
    for y in 0..h {
        let (mut row, mut row_sq) = (0u64, 0u64);
        for x in 0..w {
            let v = luma[y * w + x] as u64;
            row += v;
            row_sq += v * v;
            sum[(y + 1) * stride + x + 1] = sum[y * stride + x + 1] + row;
            sq[(y + 1) * stride + x + 1] = sq[y * stride + x + 1] + row_sq;
        }
    }
    let r = radius as usize;
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let area = ((y1 - y0) * (x1 - x0)) as f32;
            let rect = |t: &[u64]| (t[y1 * stride + x1] + t[y0 * stride + x0] - t[y0 * stride + x1] - t[y1 * stride + x0]) as f32;
            let mean = rect(&sum) / area;
            let std = (rect(&sq) / area - mean * mean).max(0.0).sqrt();
            let v = luma[y * w + x] as f32;
            // The +8 keeps flat areas from turning sensor-like noise into texture.
            out[y * w + x] = (128.0 + (v - mean) * 60.0 / (std + 8.0)).clamp(0.0, 255.0) as u8;
        }
    }
    Frame::from_luma(f.width, f.height, &out)
}

/// Global two-class threshold; the smaller class is assumed to be the text and drawn black on white.
pub fn otsu(f: &Frame) -> Frame {
    let luma = gray_levels(f).luma();
    let mut hist = [0u64; 256];
    luma.iter().for_each(|&v| hist[v as usize] += 1);
    let total: u64 = luma.len() as u64;
    let sum_all: u64 = hist.iter().enumerate().map(|(v, &n)| v as u64 * n).sum();
    let (mut w_b, mut sum_b, mut best, mut thr) = (0u64, 0u64, 0f64, 128u8);
    for t in 0..256usize {
        w_b += hist[t];
        if w_b == 0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0 {
            break;
        }
        sum_b += t as u64 * hist[t];
        let m_b = sum_b as f64 / w_b as f64;
        let m_f = (sum_all - sum_b) as f64 / w_f as f64;
        let between = w_b as f64 * w_f as f64 * (m_b - m_f).powi(2);
        if between > best {
            best = between;
            thr = t as u8;
        }
    }
    let bright = luma.iter().filter(|&&v| v > thr).count();
    let text_is_bright = bright * 2 < luma.len();
    let out: Vec<u8> = luma
        .iter()
        .map(|&v| if (v > thr) == text_is_bright { 0 } else { 255 })
        .collect();
    Frame::from_luma(f.width, f.height, &out)
}

/// Keeps only pixels brighter than `threshold` (white or yellow subtitle fill), as black ink on white.
/// Dark outlines and busy backgrounds disappear.
pub fn bright_mask(f: &Frame, threshold: u8) -> Frame {
    let out: Vec<u8> = f.luma().iter().map(|&v| if v >= threshold { 0 } else { 255 }).collect();
    Frame::from_luma(f.width, f.height, &out)
}

/// Marks pixels that differ from their neighbourhood average by more than `c`. Handles gradients and
/// uneven lighting that a single global threshold cannot. `light_ink` picks the text polarity.
pub fn adaptive(f: &Frame, radius: u32, c: i32, light_ink: bool) -> Frame {
    let luma = f.luma();
    let (w, h) = (f.width as usize, f.height as usize);
    let stride = w + 1;
    let mut integral = vec![0u32; stride * (h + 1)];
    for y in 0..h {
        let mut row = 0u32;
        for x in 0..w {
            row += luma[y * w + x] as u32;
            integral[(y + 1) * stride + x + 1] = integral[y * stride + x + 1] + row;
        }
    }
    let r = radius as usize;
    let mut out = vec![255u8; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let area = ((y1 - y0) * (x1 - x0)) as u32;
            let sum = integral[y1 * stride + x1] + integral[y0 * stride + x0]
                - integral[y0 * stride + x1]
                - integral[y1 * stride + x0];
            let mean = (sum / area) as i32;
            let v = luma[y * w + x] as i32;
            let is_ink = if light_ink { v > mean + c } else { v < mean - c };
            if is_ink {
                out[y * w + x] = 0;
            }
        }
    }
    Frame::from_luma(f.width, f.height, &out)
}

