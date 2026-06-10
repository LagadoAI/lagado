//! vision/mod.rs — In-process visual embedding via libmtmd FFI.
//!
//! Produces a mean-pooled float vector (n_embd dims, typically 1024) from a
//! PNG frame without any text-description step.  The vector goes directly into
//! MemoryTiers for cosine-similarity retrieval at query time.
//!
//! Linux only: libmtmd.so is built from the vendored llama.cpp and does not
//! exist on macOS/Windows CI.  All call sites are `#[cfg(target_os = "linux")]`.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint};
use std::sync::Mutex;

/* ── FFI declarations ────────────────────────────────────────────── */

#[repr(C)]
struct LagadoEncoder {
    _private: [u8; 0],
}

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

/* ── Public API ──────────────────────────────────────────────────── */

/// Thread-safe visual encoder.  Load once at startup, encode at episode boundaries.
pub struct VisualEncoder {
    inner: Mutex<RawEncoder>,
    pub n_embd: usize,
}

struct RawEncoder(*mut LagadoEncoder);

// SAFETY: The C encoder is not Send by default (raw pointer), but we gate all
// access behind a Mutex, and the underlying C code is single-threaded per handle
// (no global state shared between handles).
unsafe impl Send for RawEncoder {}
unsafe impl Sync for RawEncoder {}

impl Drop for RawEncoder {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { lagado_encoder_free(self.0) };
        }
    }
}

impl VisualEncoder {
    /// Load the VLM text model + mmproj projector.
    /// `use_gpu`: pass true to offload the vision encoder to GPU.
    pub fn load(model_path: &str, mmproj_path: &str, use_gpu: bool) -> Result<Self, String> {
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
        Ok(Self {
            inner: Mutex::new(RawEncoder(ptr)),
            n_embd,
        })
    }

    /// Encode a PNG image to a mean-pooled embedding vector.
    ///
    /// Returns `None` on decode failure, empty bytes, or encoder error.
    /// The returned Vec has exactly `self.n_embd` elements.
    pub fn encode_png(&self, png_bytes: &[u8]) -> Option<Vec<f32>> {
        if png_bytes.is_empty() {
            return None;
        }

        // Decode PNG → raw RGB (3 channels, no alpha)
        let img = image::load_from_memory(png_bytes)
            .map_err(|e| tracing::warn!("PNG decode failed: {e}"))
            .ok()?;
        let rgb = img.to_rgb8();
        let (nx, ny) = rgb.dimensions();
        let rgb_data = rgb.as_raw();

        let mut out = vec![0.0f32; self.n_embd];

        let guard = self.inner.lock().unwrap();
        let ret = unsafe {
            lagado_encode_image(
                guard.0,
                rgb_data.as_ptr(),
                nx,
                ny,
                out.as_mut_ptr(),
            )
        };
        drop(guard);

        if ret <= 0 {
            tracing::warn!("lagado_encode_image returned {ret}");
            return None;
        }

        Some(out)
    }
}

/// Cosine similarity between two equal-length float slices. Returns 0.0 on
/// zero-norm inputs rather than NaN.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
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
}
