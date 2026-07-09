//! perception/canvas.rs — reader for the shared-memory LIVE CANVAS (the capture path).
//!
//! The membrane feed (reflex/membrane/canvas_feed.py) maintains a raw BGRX buffer in
//! /dev/shm, patched rect-by-rect from the RFB pixel stream at repaint cadence. This
//! reader replaces the FRAME_PATH PNG round trip in the live loop: no screendump, no
//! encode, no decode — one read of raw bytes plus a BGRX→RGB swizzle (which the CV
//! sense needs), and NOTHING else.
//!
//! Layout: magic "LGCV" | u32 w | u32 h | u32 stride | u64 seq (LE), then h*stride
//! BGRX bytes. `seq` bumps per rect batch — callers can skip work on an unchanged
//! canvas. Fail-open: any error returns None and the caller falls back to the legacy
//! PNG path (the canvas is additive, never a new single point of failure).

use std::io::Read;

const MAGIC: &[u8; 4] = b"LGCV";
const HDR: usize = 24;

pub fn canvas_path() -> String {
    std::env::var("LAGADO_CANVAS").unwrap_or_else(|_| "/dev/shm/lagado_canvas".to_string())
}

/// Header + liveness probe without reading pixels.
pub fn canvas_seq() -> Option<(u32, u32, u64)> {
    let mut f = std::fs::File::open(canvas_path()).ok()?;
    let mut hdr = [0u8; HDR];
    f.read_exact(&mut hdr).ok()?;
    if &hdr[..4] != MAGIC {
        return None;
    }
    let w = u32::from_le_bytes(hdr[4..8].try_into().ok()?);
    let h = u32::from_le_bytes(hdr[8..12].try_into().ok()?);
    let seq = u64::from_le_bytes(hdr[16..24].try_into().ok()?);
    Some((w, h, seq))
}

/// Read the live canvas as packed RGB8 (the format `cv_proposer::propose_frame`
/// consumes). One copy + swizzle; zero codecs. None → fall back to the PNG path.
pub fn read_rgb() -> Option<(Vec<u8>, u32, u32)> {
    let mut f = std::fs::File::open(canvas_path()).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    if buf.len() < HDR || &buf[..4] != MAGIC {
        return None;
    }
    let w = u32::from_le_bytes(buf[4..8].try_into().ok()?) as usize;
    let h = u32::from_le_bytes(buf[8..12].try_into().ok()?) as usize;
    let stride = u32::from_le_bytes(buf[12..16].try_into().ok()?) as usize;
    if w == 0 || h == 0 || buf.len() < HDR + h * stride {
        return None;
    }
    let mut rgb = Vec::with_capacity(w * h * 3);
    for row in 0..h {
        let base = HDR + row * stride;
        for col in 0..w {
            let px = base + col * 4;
            rgb.push(buf[px + 2]); // R  (canvas is BGRX)
            rgb.push(buf[px + 1]); // G
            rgb.push(buf[px]);     // B
        }
    }
    Some((rgb, w as u32, h as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_canvas(path: &str, w: u32, h: u32, seq: u64) {
        let stride = w * 4;
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(MAGIC).unwrap();
        f.write_all(&w.to_le_bytes()).unwrap();
        f.write_all(&h.to_le_bytes()).unwrap();
        f.write_all(&stride.to_le_bytes()).unwrap();
        f.write_all(&seq.to_le_bytes()).unwrap();
        // 2x2: blue, green, red, white in BGRX
        let px: &[[u8; 4]] = &[[255, 0, 0, 0], [0, 255, 0, 0], [0, 0, 255, 0], [255, 255, 255, 0]];
        for p in px.iter().take((w * h) as usize) {
            f.write_all(p).unwrap();
        }
    }

    #[test]
    fn canvas_roundtrip_and_fail_open() {
        // ONE test: LAGADO_CANVAS is process-global; parallel tests race it.
        std::env::set_var("LAGADO_CANVAS", "/nonexistent/lagado_zzz");
        assert!(read_rgb().is_none());
        assert!(canvas_seq().is_none());
        let path = std::env::temp_dir().join("lagado_canvas_test");
        let path = path.to_str().unwrap().to_string();
        write_canvas(&path, 2, 2, 7);
        std::env::set_var("LAGADO_CANVAS", &path);
        let (w, h, seq) = canvas_seq().expect("header");
        assert_eq!((w, h, seq), (2, 2, 7));
        let (rgb, w, h) = read_rgb().expect("pixels");
        assert_eq!((w, h), (2, 2));
        assert_eq!(&rgb[0..3], &[0, 0, 255]);      // blue in RGB
        assert_eq!(&rgb[3..6], &[0, 255, 0]);      // green
        assert_eq!(&rgb[6..9], &[255, 0, 0]);      // red
        assert_eq!(&rgb[9..12], &[255, 255, 255]); // white
        std::env::remove_var("LAGADO_CANVAS");
    }
}
