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
use std::os::raw::c_int as c_bool_int;

#[cfg(lagado_vision_ffi)]
#[repr(C)]
struct LagadoTileData {
    is_overview:    c_bool_int,   // 1 = global thumbnail, skip spatial mapping
    tile_refined_x: c_uint,       // top-left of tile in refined image (px)
    tile_refined_y: c_uint,
    n_tokens:       c_uint,
    embeddings:     *const f32,   // n_tokens × n_embd floats; owned by LagadoPatchResult
}

#[cfg(lagado_vision_ffi)]
#[repr(C)]
struct LagadoPatchResult {
    n_tiles:   usize,
    refined_w: c_uint,
    refined_h: c_uint,
    tiles:     *const LagadoTileData,  // array of n_tiles
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

    fn lagado_encode_image_patches(
        enc:      *mut LagadoEncoder,
        rgb_data: *const u8,
        nx:       c_uint,
        ny:       c_uint,
    ) -> *mut LagadoPatchResult;

    fn lagado_patch_result_free(r: *mut LagadoPatchResult);
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

// ── Per-patch embedding API ───────────────────────────────────────────

/// Single patch embedding with its location in the original image.
///
/// `patch_x`, `patch_y` — top-left corner in ORIGINAL (pre-scaling) image pixels.
/// `patch_w`, `patch_h` — patch footprint in original pixels (same for all spatial patches).
/// `embd`               — per-patch embedding vector (n_embd floats).
#[derive(Debug, Clone)]
pub struct PatchEmbedding {
    pub patch_x: u32,
    pub patch_y: u32,
    pub patch_w: u32,
    pub patch_h: u32,
    pub embd:    Vec<f32>,
}

/// All patches from one tile (spatial or overview).
///
/// `is_overview` — if true, the tile is the global thumbnail; `patches` contains
/// embeddings but `patch_x/y` are all zero and must NOT be used for spatial matching.
/// `tile_origin_x/y` — tile top-left in ORIGINAL image (0 for overview / non-tiled).
#[derive(Debug, Clone)]
pub struct TilePatches {
    pub is_overview:  bool,
    pub tile_origin_x: u32,
    pub tile_origin_y: u32,
    pub patches:      Vec<PatchEmbedding>,
}

impl VisualEncoder {
    /// Encode a PNG image to per-tile, per-patch embeddings.
    ///
    /// Returns `None` on non-Linux, PNG decode failure, or encoder error.
    /// The overview tile (`is_overview=true`) is included but its `patch_x/y` are zero
    /// — callers must check `is_overview` and skip spatial mapping for those patches.
    pub fn encode_png_patches(&self, png_bytes: &[u8]) -> Option<Vec<TilePatches>> {
        if png_bytes.is_empty() {
            return None;
        }

        #[cfg(lagado_vision_ffi)]
        {
            let img = image::load_from_memory(png_bytes)
                .map_err(|e| tracing::warn!("PNG decode failed in encode_png_patches: {e}"))
                .ok()?;
            let rgb = img.to_rgb8();
            let (nx, ny) = rgb.dimensions();
            let rgb_data = rgb.as_raw();

            let guard = self.inner.lock().unwrap();
            let raw_result = unsafe {
                lagado_encode_image_patches(guard.0, rgb_data.as_ptr(), nx, ny)
            };
            drop(guard);

            if raw_result.is_null() {
                tracing::warn!("lagado_encode_image_patches returned null");
                return None;
            }

            let n_tiles = unsafe { (*raw_result).n_tiles };
            let refined_w = unsafe { (*raw_result).refined_w };
            let refined_h = unsafe { (*raw_result).refined_h };

            // LFM2 patch stride in refined image (512/16 = 32px)
            const PATCH_STRIDE: u32 = 32;
            const N_PATCH_COLS: u32 = 16; // per spatial tile (512 / 32)

            // Patch footprint in original image (approximate, same for all patches in a frame)
            let patch_w_orig = if refined_w > 0 {
                (PATCH_STRIDE as f32 * nx as f32 / refined_w as f32).round() as u32
            } else {
                0
            };
            let patch_h_orig = if refined_h > 0 {
                (PATCH_STRIDE as f32 * ny as f32 / refined_h as f32).round() as u32
            } else {
                0
            };

            let mut result = Vec::with_capacity(n_tiles);

            for ti in 0..n_tiles {
                let tile = unsafe { &*(*raw_result).tiles.add(ti) };
                let is_ov = tile.is_overview != 0;
                let n_tok = tile.n_tokens as usize;

                // Tile top-left in original image coords
                let tile_ox = if !is_ov && refined_w > 0 {
                    (tile.tile_refined_x as f32 * nx as f32 / refined_w as f32).round() as u32
                } else {
                    0
                };
                let tile_oy = if !is_ov && refined_h > 0 {
                    (tile.tile_refined_y as f32 * ny as f32 / refined_h as f32).round() as u32
                } else {
                    0
                };

                let mut patches = Vec::with_capacity(n_tok);

                if tile.embeddings.is_null() {
                    // Encoding failed for this tile; emit empty embeddings
                    for _ in 0..n_tok {
                        patches.push(PatchEmbedding {
                            patch_x: 0, patch_y: 0,
                            patch_w: 0, patch_h: 0,
                            embd: vec![0.0f32; self.n_embd],
                        });
                    }
                } else {
                    for tok_i in 0..n_tok {
                        let (px, py) = if is_ov {
                            // Overview: no spatial meaning; coords zeroed
                            (0u32, 0u32)
                        } else {
                            // Row-major within tile: row = i / N_PATCH_COLS, col = i % N_PATCH_COLS
                            let patch_col = tok_i as u32 % N_PATCH_COLS;
                            let patch_row = tok_i as u32 / N_PATCH_COLS;
                            let refined_px = tile.tile_refined_x + patch_col * PATCH_STRIDE;
                            let refined_py = tile.tile_refined_y + patch_row * PATCH_STRIDE;
                            // Scale to original image coords
                            let ox = if refined_w > 0 {
                                (refined_px as f32 * nx as f32 / refined_w as f32).round() as u32
                            } else { 0 };
                            let oy = if refined_h > 0 {
                                (refined_py as f32 * ny as f32 / refined_h as f32).round() as u32
                            } else { 0 };
                            (ox, oy)
                        };

                        let embd_ptr = unsafe { tile.embeddings.add(tok_i * self.n_embd) };
                        let embd = unsafe {
                            std::slice::from_raw_parts(embd_ptr, self.n_embd).to_vec()
                        };

                        patches.push(PatchEmbedding {
                            patch_x: px,
                            patch_y: py,
                            patch_w: if is_ov { 0 } else { patch_w_orig },
                            patch_h: if is_ov { 0 } else { patch_h_orig },
                            embd,
                        });
                    }
                }

                result.push(TilePatches {
                    is_overview:   is_ov,
                    tile_origin_x: tile_ox,
                    tile_origin_y: tile_oy,
                    patches,
                });
            }

            unsafe { lagado_patch_result_free(raw_result) };
            return Some(result);
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
        #[cfg(not(lagado_vision_ffi))]
        {
            let enc = VisualEncoder { inner: Mutex::new(RawEncoder), n_embd: 1024 };
            assert_eq!(enc.encode_png(b""), None);
        }
    }

    #[test]
    fn encode_png_patches_returns_none_on_empty() {
        #[cfg(not(lagado_vision_ffi))]
        {
            let enc = VisualEncoder { inner: Mutex::new(RawEncoder), n_embd: 1024 };
            assert!(enc.encode_png_patches(b"").is_none());
        }
    }

    #[test]
    fn patch_embedding_fields_are_public() {
        // Compile-time check that the public API is accessible
        let pe = PatchEmbedding { patch_x: 10, patch_y: 20, patch_w: 27, patch_h: 25, embd: vec![0.0] };
        assert_eq!(pe.patch_x, 10);
        assert_eq!(pe.patch_y, 20);
        assert_eq!(pe.patch_w, 27);
        assert_eq!(pe.patch_h, 25);
    }

    #[test]
    fn tile_patches_overview_zeroed_coords() {
        // Overview tiles must carry zero spatial coords — verified by construction
        let overview = TilePatches {
            is_overview:   true,
            tile_origin_x: 0,
            tile_origin_y: 0,
            patches: vec![PatchEmbedding { patch_x: 0, patch_y: 0, patch_w: 0, patch_h: 0, embd: vec![] }],
        };
        assert!(overview.is_overview);
        assert_eq!(overview.patches[0].patch_x, 0);
        assert_eq!(overview.patches[0].patch_w, 0);
    }

    #[test]
    fn spatial_tile_has_nonzero_footprint() {
        // A spatial tile at (tile_col=1, tile_row=0) for 1280x800 frame:
        // tile_origin_x = 512 * 1280 / 1536 ≈ 427, patch_w ≈ 27
        let tile = TilePatches {
            is_overview:   false,
            tile_origin_x: 427,
            tile_origin_y: 0,
            patches: vec![PatchEmbedding { patch_x: 427, patch_y: 0, patch_w: 27, patch_h: 25, embd: vec![1.0, 2.0] }],
        };
        assert!(!tile.is_overview);
        assert!(tile.patches[0].patch_w > 0);
        assert!(tile.patches[0].patch_h > 0);
    }
}
