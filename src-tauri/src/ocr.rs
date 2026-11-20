use serde::Serialize;
use windows::{
    core::HSTRING,
    Globalization::Language,
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Storage::Streams::DataWriter,
};

#[derive(Clone, Serialize)]
pub struct OcrLang {
    pub tag: String,
    pub name: String,
}

pub struct Line {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Several adjacent OCR lines merged so they are translated as one sentence.
#[derive(Clone)]
pub struct Block {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub lines: usize,
}

/// OCR languages that have a Windows language pack installed.
pub fn available_languages() -> Vec<OcrLang> {
    OcrEngine::AvailableRecognizerLanguages()
        .map(|langs| {
            langs
                .into_iter()
                .filter_map(|l| {
                    Some(OcrLang {
                        tag: l.LanguageTag().ok()?.to_string(),
                        name: l.DisplayName().ok()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn create_engine(tag: &str) -> windows::core::Result<OcrEngine> {
    if tag.is_empty() || tag == "auto" {
        OcrEngine::TryCreateFromUserProfileLanguages()
    } else {
        OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from(tag))?)
    }
}

pub fn recognize(
    engine: &OcrEngine,
    bgra: &[u8],
    width: u32,
    height: u32,
) -> windows::core::Result<Vec<Line>> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(bgra)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap =
        SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width as i32, height as i32)?;
    let result = engine.RecognizeAsync(&bitmap)?.get()?;

    let mut lines = Vec::new();
    for line in result.Lines()? {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, 0f32, 0f32);
        for word in line.Words()? {
            let r = word.BoundingRect()?;
            x0 = x0.min(r.X);
            y0 = y0.min(r.Y);
            x1 = x1.max(r.X + r.Width);
            y1 = y1.max(r.Y + r.Height);
        }
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        lines.push(Line { text: line.Text()?.to_string(), x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
    }
    Ok(lines)
}

/// Windows OCR puts spaces between CJK characters; scripts written without spaces need them removed.
pub fn is_spaceless_script(tag: &str) -> bool {
    let t = tag.to_ascii_lowercase();
    t.starts_with("ja") || t.starts_with("zh")
}

pub fn group_lines(lines: Vec<Line>, spaceless: bool) -> Vec<Block> {
    let sep = if spaceless { "" } else { " " };
    let mut blocks: Vec<Block> = Vec::new();

    for line in lines {
        let text = if spaceless { line.text.replace(' ', "") } else { line.text.clone() };
        let joined = blocks.iter_mut().find(|b| {
            let line_h = b.h / b.lines as f32;
            let gap = line.y - (b.y + b.h);
            let near_vertically = gap > -line_h * 0.5 && gap < line_h * 0.9;
            let left_aligned = (line.x - b.x).abs() < line_h * 2.0;
            let centred = ((line.x + line.w / 2.0) - (b.x + b.w / 2.0)).abs() < line_h * 2.0;
            near_vertically && (left_aligned || centred)
        });

        match joined {
            Some(b) => {
                b.text = format!("{}{}{}", b.text, sep, text);
                let right = (b.x + b.w).max(line.x + line.w);
                let bottom = (b.y + b.h).max(line.y + line.h);
                b.x = b.x.min(line.x);
                b.w = right - b.x;
                b.h = bottom - b.y;
                b.lines += 1;
            }
            None => blocks.push(Block { text, x: line.x, y: line.y, w: line.w, h: line.h, lines: 1 }),
        }
    }
    blocks
}
