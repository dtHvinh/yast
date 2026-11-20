use std::{
    error::Error,
    sync::mpsc::SyncSender,
    time::{Duration, Instant},
};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
};

/// A cropped BGRA frame ready for OCR, plus where it came from in the full window.
pub struct Shot {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub offset_x: u32,
    pub offset_y: u32,
    pub full_width: u32,
    pub full_height: u32,
}

pub struct Flags {
    /// Called when the captured window goes away.
    pub on_closed: Box<dyn Fn() + Send>,
    pub tx: SyncSender<Shot>,
    pub interval: Duration,
    pub region: [f32; 4],
}

pub struct Capture {
    on_closed: Box<dyn Fn() + Send>,
    tx: SyncSender<Shot>,
    interval: Duration,
    region: [f32; 4],
    last_frame: Instant,
    last_hash: u64,
}

impl GraphicsCaptureApiHandler for Capture {
    type Flags = Flags;
    type Error = Box<dyn Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let f = ctx.flags;
        Ok(Self {
            on_closed: f.on_closed,
            tx: f.tx,
            interval: f.interval,
            region: f.region,
            last_frame: Instant::now() - f.interval,
            last_hash: 0,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.last_frame.elapsed() < self.interval {
            return Ok(());
        }
        self.last_frame = Instant::now();

        let mut buffer = frame.buffer()?;
        let (full_w, full_h) = (buffer.width(), buffer.height());
        let pixels = buffer.as_nopadding_buffer()?;

        let [rx, ry, rw, rh] = self.region;
        let x0 = ((rx * full_w as f32) as u32).min(full_w.saturating_sub(1));
        let y0 = ((ry * full_h as f32) as u32).min(full_h.saturating_sub(1));
        let w = ((rw * full_w as f32) as u32).clamp(1, full_w - x0);
        let h = ((rh * full_h as f32) as u32).clamp(1, full_h - y0);
        let stride = full_w as usize * 4;

        // Skip OCR when the region looks the same as last time (every 4th pixel of every 4th row).
        let mut hash: u64 = 0xcbf29ce484222325;
        for row in (y0..y0 + h).step_by(4) {
            let base = row as usize * stride + x0 as usize * 4;
            for px in (0..w as usize).step_by(4) {
                let i = base + px * 4;
                let v = u32::from_le_bytes([pixels[i], pixels[i + 1], pixels[i + 2], 0]);
                hash = (hash ^ v as u64).wrapping_mul(0x100000001b3);
            }
        }
        if hash == self.last_hash {
            return Ok(());
        }
        self.last_hash = hash;

        let mut data = Vec::with_capacity(w as usize * h as usize * 4);
        for row in y0..y0 + h {
            let start = row as usize * stride + x0 as usize * 4;
            data.extend_from_slice(&pixels[start..start + w as usize * 4]);
        }

        // If OCR is still busy with the previous frame, drop this one; the next change will be picked up.
        let _ = self.tx.try_send(Shot {
            data,
            width: w,
            height: h,
            offset_x: x0,
            offset_y: y0,
            full_width: full_w,
            full_height: full_h,
        });
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        (self.on_closed)();
        Ok(())
    }
}
