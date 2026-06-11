//! vision/mod.rs — Visual embedding via in-process libmtmd FFI.
//!
//! Public API compiles everywhere. `VisualEncoder::load()` returns Err on non-Linux
//! (libmtmd.so is a Linux-only build artifact). Callers hold `Option<Arc<VisualEncoder>>`
//! and receive None on non-Linux — no platform cfg gates needed at call sites.

use std::sync::Mutex;

// ── FFI block (Linux only) ────────────────────────────────────────

#[cfg(lagado_vision_ffi)]
use std::ffi::CString;
#[cfg(lagado_vision_ffi)]
use std::os::raw::{c_char, c_int, c_uint};

#[cfg(lagado_vision_ffi)]
#[repr(C)]
struct LagadoEncoder {
    _private: [u8; 0],
}

#[cfg(lagado_vision_ffi)]
extern "C" {
    fn lagado_encoder_init(
        model_path:  *const c_char,
        mmproj_path: *const c_char,
        use_gpu:     c_int,
    ) -> *mut LagadoEncoder;

    fn lagado_encoder_n_embd(enc: *const LagadoEncoder) -> i32;

    fn lagado_encode_image(
        enc:      *mut LagadoEncoder,
        rgb_data: *const u8,
        nx:       c_uint,
        ny:       c_uint,
        out_embd: *mut f32,
    ) -> i32;

    fn lagado_encoder_free(enc: *mut LagadoEncoder);
}

// ── RawEncoder — platform-specific pointer wrapper ────────────────

// Two struct definitions, one per platform — same name, safe on both.
#[cfg(lagado_vision_ffi)]
struct RawEncoder(*mut LagadoEncoder);

#[cfg(not(lagado_vision_ffi))]
struct RawEncoder;

// SAFETY: access is gated behind Mutex; C encoder has no global mutable state per handle.
unsafe impl Send for RawEncoder {}
unsafe impl Sync for RawEncoder {}

impl Drop for RawEncoder {
    fn drop(&mut self) {
        #[cfg(lagado_vision_ffi)]
        if !self.0.is_null() {
            unsafe { lagado_encoder_free(self.0) };
        }
    }
}

// ── Public API ────────────────────────────────────────────────────

/// Thread-safe visual encoder. Load once at startup; encode at episode boundaries.
/// Always present as a type — `load()` returns Err on non-Linux.
pub struct VisualEncoder {
    inner: Mutex<RawEncoder>,
    pub n_embd: usize,
}

impl VisualEncoder {
    /// Load the VLM text model + mmproj projector.
    /// Returns Err on non-Linux platforms (no libmtmd.so).
    pub fn load(model_path: &str, mmproj_path: &str, use_gpu: bool) -> Result<Self, String> {
        #[cfg(lagado_vision_ffi)]
        {
            let c_model  = CString::new(model_path).map_err(|e| e.to_string())?;
            let c_mmproj = CString::new(mmproj_path).map_err(|e| e.to_string())?;
            let gpu_flag = if use_gpu { 1 } else { 0 };

            let ptr = unsafe {
                lagado_encoder_init(c_model.as_ptr(), c_mmproj.as_ptr(), gpu_flag)
            };

            if ptr.is_null() {
                return Err(format!(
                    "lagado_encoder_init failed — model={model_path} mmproj={mmproj_path}"
                ));
            }

            let n_embd = unsafe { lagado_encoder_n_embd(ptr) } as usize;
            if n_embd == 0 {
                unsafe { lagado_encoder_free(ptr) };
                return Err("model reports n_embd=0".to_string());
            }

            tracing::info!("VisualEncoder loaded: n_embd={n_embd}");
            return Ok(Self {
                inner: Mutex::new(RawEncoder(ptr)),
                n_embd,
            });
        }
        #[cfg(not(lagado_vision_ffi))]
        Err("visual encoding requires Linux (libmtmd.so not available on this platform)".to_string())
    }

    /// Encode a PNG image to a mean-pooled embedding vector.
    /// Returns None on non-Linux, decode failure, or encoder error.
    pub fn encode_png(&self, png_bytes: &[u8]) -> Option<Vec<f32>> {
        if png_bytes.is_empty() {
            return None;
        }

        #[cfg(lagado_vision_ffi)]
        {
            let img = image::load_from_memory(png_bytes)
                .map_err(|e| tracing::warn!("PNG decode failed: {e}"))
                .ok()?;
            let rgb = img.to_rgb8();
            let (nx, ny) = rgb.dimensions();
            let rgb_data = rgb.as_raw();

            let mut out = vec![0.0f32; self.n_embd];
            let guard = self.inner.lock().unwrap();
            let ret = unsafe {
                lagado_encode_image(guard.0, rgb_data.as_ptr(), nx, ny, out.as_mut_ptr())
            };
            drop(guard);

            if ret <= 0 {
                tracing::warn!("lagado_encode_image returned {ret}");
                return None;
            }
            return Some(out);
        }
        #[cfg(not(lagado_vision_ffi))]
        { let _ = png_bytes; None }
    }
}

/// Cosine similarity between two equal-length float slices.
/// Returns 0.0 on zero-norm inputs rather than NaN.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let dot: f32  = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { return 0.0; }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn cosine_zero_safe() {
        let z = vec![0.0, 0.0];
        let v = vec![1.0, 0.0];
        assert_eq!(cosine_similarity(&z, &v), 0.0);
    }

    #[test]
    fn load_returns_err_on_bad_path() {
        let r = VisualEncoder::load("/nonexistent/model.gguf", "/nonexistent/mmproj.gguf", false);
        assert!(r.is_err());
    }

    #[test]
    fn encode_png_returns_none_on_empty() {
        // Can't construct a real VisualEncoder without model files,
        // but encode_png must return None for empty bytes regardless.
        #[cfg(not(lagado_vision_ffi))]
        {
            let enc = VisualEncoder { inner: Mutex::new(RawEncoder), n_embd: 1024 };
            assert_eq!(enc.encode_png(b""), None);
        }
    }
}
