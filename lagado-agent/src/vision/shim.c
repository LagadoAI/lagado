/*
 * shim.c — thin C wrapper over llama.cpp + libmtmd for in-process visual encoding.
 *
 * Exposes a minimal, pointer-only API so Rust FFI never has to pass C structs
 * by value (which is ABI-fragile across compiler versions).
 *
 * Lifetime contract:
 *   lagado_encoder_init()  — allocates; caller owns the returned handle
 *   lagado_encode_image()  — encodes one PNG frame; caller provides output buffer
 *   lagado_encoder_free()  — frees model, mtmd context, and handle
 *
 * Thread safety: lagado_encode_image() is NOT thread-safe per handle.
 * The Rust side holds the encoder behind a Mutex.
 */

#include "llama.h"
#include "mtmd.h"
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdbool.h>

typedef struct {
    struct llama_model *model;
    mtmd_context       *mtmd_ctx;
    int32_t             n_embd;
} lagado_encoder_t;

/* ── Init ─────────────────────────────────────────────────────────── */

lagado_encoder_t *lagado_encoder_init(const char *model_path,
                                      const char *mmproj_path,
                                      int         use_gpu)
{
    llama_backend_init();

    struct llama_model_params mp = llama_model_default_params();
    mp.n_gpu_layers = use_gpu ? 32 : 0;

    struct llama_model *model = llama_model_load_from_file(model_path, mp);
    if (!model) {
        fprintf(stderr, "[lagado_vision] failed to load model: %s\n", model_path);
        return NULL;
    }

    struct mtmd_context_params cp = mtmd_context_params_default();
    cp.use_gpu       = (bool)use_gpu;
    cp.print_timings = false;
    cp.n_threads     = 4;
    cp.warmup        = false;

    mtmd_context *ctx = mtmd_init_from_file(mmproj_path, model, cp);
    if (!ctx) {
        fprintf(stderr, "[lagado_vision] failed to load mmproj: %s\n", mmproj_path);
        llama_model_free(model);
        return NULL;
    }

    lagado_encoder_t *enc = (lagado_encoder_t *)malloc(sizeof(lagado_encoder_t));
    if (!enc) {
        mtmd_free(ctx);
        llama_model_free(model);
        return NULL;
    }

    enc->model    = model;
    enc->mtmd_ctx = ctx;
    enc->n_embd   = llama_model_n_embd_inp(model);
    return enc;
}

int32_t lagado_encoder_n_embd(const lagado_encoder_t *enc)
{
    return enc ? enc->n_embd : 0;
}

/* ── Encode ───────────────────────────────────────────────────────── */

/*
 * Encode one image frame to a mean-pooled embedding vector.
 *
 * rgb_data — raw RGB bytes (nx * ny * 3), NOT RGBA.
 * out_embd — pre-allocated buffer of n_embd floats.
 *
 * Returns n_embd on success, -1 on any failure.
 */
int32_t lagado_encode_image(lagado_encoder_t   *enc,
                             const uint8_t      *rgb_data,
                             uint32_t            nx,
                             uint32_t            ny,
                             float              *out_embd)
{
    if (!enc || !rgb_data || !out_embd || nx == 0 || ny == 0)
        return -1;

    mtmd_bitmap *bmp = mtmd_bitmap_init(nx, ny, rgb_data);
    if (!bmp) return -1;

    mtmd_input_chunks *chunks = mtmd_input_chunks_init();
    if (!chunks) { mtmd_bitmap_free(bmp); return -1; }

    /* Tokenize: one image marker → three chunks (prefix, image, suffix) */
    const char *marker = mtmd_default_marker();
    struct mtmd_input_text input_text = { marker, false, true };
    const mtmd_bitmap *bitmaps[1] = { bmp };

    int ret = mtmd_tokenize(enc->mtmd_ctx, chunks, &input_text, bitmaps, 1);
    if (ret != 0) {
        mtmd_input_chunks_free(chunks);
        mtmd_bitmap_free(bmp);
        return -1;
    }

    /* Find the image chunk, encode it, mean-pool the output */
    size_t  n_chunks      = mtmd_input_chunks_size(chunks);
    int32_t n_embd_tokens = 0;
    float  *embd_sum      = (float *)calloc((size_t)enc->n_embd, sizeof(float));
    if (!embd_sum) {
        mtmd_input_chunks_free(chunks);
        mtmd_bitmap_free(bmp);
        return -1;
    }

    for (size_t i = 0; i < n_chunks; i++) {
        const mtmd_input_chunk *chunk = mtmd_input_chunks_get(chunks, i);
        if (mtmd_input_chunk_get_type(chunk) != MTMD_INPUT_CHUNK_TYPE_IMAGE)
            continue;

        ret = mtmd_encode_chunk(enc->mtmd_ctx, chunk);
        if (ret != 0) {
            free(embd_sum);
            mtmd_input_chunks_free(chunks);
            mtmd_bitmap_free(bmp);
            return -1;
        }

        float   *embd    = mtmd_get_output_embd(enc->mtmd_ctx);
        int32_t  n_toks  = (int32_t)mtmd_input_chunk_get_n_tokens(chunk);

        /* Sum across token dimension */
        for (int32_t t = 0; t < n_toks; t++) {
            const float *tok_embd = embd + (size_t)t * (size_t)enc->n_embd;
            for (int32_t d = 0; d < enc->n_embd; d++)
                embd_sum[d] += tok_embd[d];
        }
        n_embd_tokens += n_toks;
        break; /* one image chunk expected */
    }

    if (n_embd_tokens == 0) {
        free(embd_sum);
        mtmd_input_chunks_free(chunks);
        mtmd_bitmap_free(bmp);
        return -1;
    }

    /* Divide to get mean */
    float inv_n = 1.0f / (float)n_embd_tokens;
    for (int32_t d = 0; d < enc->n_embd; d++)
        out_embd[d] = embd_sum[d] * inv_n;

    free(embd_sum);
    mtmd_input_chunks_free(chunks);
    mtmd_bitmap_free(bmp);
    return enc->n_embd;
}

/* ── Free ─────────────────────────────────────────────────────────── */

void lagado_encoder_free(lagado_encoder_t *enc)
{
    if (!enc) return;
    if (enc->mtmd_ctx) mtmd_free(enc->mtmd_ctx);
    if (enc->model)    llama_model_free(enc->model);
    llama_backend_free();
    free(enc);
}

/* ── Per-patch encoding ───────────────────────────────────────────── */
/*
 * LFM2.5-VL tiling constants (from tools/mtmd/mtmd-image.h).
 * A 1280×800 production frame tiles into a 3×2 grid of 512×512 spatial
 * tiles plus one global-thumbnail overview chunk (placed LAST).
 *
 * Empirically verified (probe_ordering.c, 2026-06-11):
 *   - Overview is the LAST image chunk (not first).
 *   - Spatial tiles are in row-major order: chunk idx = tile_row*grid_cols + tile_col.
 *   - Within a tile, token i is row-major: row = i/16, col = i%16.
 *   - Overview identification: structural position img_idx >= grid_cols*grid_rows.
 *     (NOT by token count — at certain resolutions the overview can have 256 tokens
 *      identical to spatial tiles, e.g. a 1025×1025 square input with a 2×2 grid.)
 */

#define LFM2_TILE_SIZE        512
#define LFM2_PATCH_STRIDE     32      /* patch_size(16) × n_merge(2) */
#define LFM2_MIN_TILES        2
#define LFM2_MAX_TILES        10
#define LFM2_MAX_TOL          2.0f

typedef struct {
    int      is_overview;       /* 1 = global thumbnail, skip spatial mapping */
    uint32_t tile_refined_x;    /* top-left of tile in refined image (px) */
    uint32_t tile_refined_y;
    uint32_t n_tokens;
    float   *embeddings;        /* n_tokens × n_embd floats; owned by lagado_patch_result_t */
} lagado_tile_data_t;

typedef struct {
    size_t              n_tiles;
    uint32_t            refined_w;  /* full refined image dimensions */
    uint32_t            refined_h;
    lagado_tile_data_t *tiles;      /* array of n_tiles, owned here */
} lagado_patch_result_t;

/* Replicate mtmd_image_preprocessor_lfm2::get_grid_layout from tools/mtmd/mtmd-image.cpp.
 * Returns (grid_cols, grid_rows) = the closest aspect-ratio-matching tile grid. */
static void lfm2_find_grid(uint32_t nx, uint32_t ny, int *grid_cols, int *grid_rows)
{
    float aspect    = (float)nx / (float)ny;
    float best_diff = 1e30f;
    *grid_cols = 1;
    *grid_rows = 1;

    for (int n = LFM2_MIN_TILES; n <= LFM2_MAX_TILES; n++) {
        for (int w = 1; w <= n; w++) {
            for (int h = 1; h <= n; h++) {
                int prod = w * h;
                if (prod < LFM2_MIN_TILES || prod > LFM2_MAX_TILES)
                    continue;
                float ga   = (float)w / (float)h;
                float diff = fabsf(aspect - ga);
                if (diff < best_diff) {
                    best_diff  = diff;
                    *grid_cols = w;
                    *grid_rows = h;
                } else if (diff == best_diff) {
                    float target_area = (float)(LFM2_TILE_SIZE * LFM2_TILE_SIZE * prod);
                    float actual_area = (float)nx * (float)ny;
                    if (actual_area > 0.5f * target_area) {
                        *grid_cols = w;
                        *grid_rows = h;
                    }
                }
            }
        }
    }
}

/*
 * Encode an image to per-tile, per-patch embeddings.
 *
 * Returns a heap-allocated lagado_patch_result_t on success, NULL on failure.
 * Caller must free with lagado_patch_result_free().
 *
 * For each tile: is_overview identifies the global thumbnail (no reliable spatial
 * correspondence). Spatial tiles carry tile_refined_x/y — top-left in the refined
 * image — so the Rust layer can compute original-image pixel coords.
 */
lagado_patch_result_t *lagado_encode_image_patches(lagado_encoder_t   *enc,
                                                    const uint8_t      *rgb_data,
                                                    uint32_t            nx,
                                                    uint32_t            ny)
{
    if (!enc || !rgb_data || nx == 0 || ny == 0) return NULL;

    int needs_tiling = ((float)nx > LFM2_TILE_SIZE * LFM2_MAX_TOL) ||
                       ((float)ny > LFM2_TILE_SIZE * LFM2_MAX_TOL);

    int grid_cols = 1, grid_rows = 1;
    uint32_t refined_w = 0, refined_h = 0;
    if (needs_tiling) {
        lfm2_find_grid(nx, ny, &grid_cols, &grid_rows);
        refined_w = (uint32_t)(LFM2_TILE_SIZE * grid_cols);
        refined_h = (uint32_t)(LFM2_TILE_SIZE * grid_rows);
    }

    mtmd_bitmap *bmp = mtmd_bitmap_init(nx, ny, rgb_data);
    if (!bmp) return NULL;

    mtmd_input_chunks *chunks = mtmd_input_chunks_init();
    if (!chunks) { mtmd_bitmap_free(bmp); return NULL; }

    const char *marker = mtmd_default_marker();
    struct mtmd_input_text it = { marker, false, true };
    const mtmd_bitmap *bitmaps[1] = { bmp };

    int ret = mtmd_tokenize(enc->mtmd_ctx, chunks, &it, bitmaps, 1);
    if (ret != 0) {
        mtmd_input_chunks_free(chunks);
        mtmd_bitmap_free(bmp);
        return NULL;
    }

    /* Count image chunks */
    size_t n_total = mtmd_input_chunks_size(chunks);
    size_t n_img   = 0;
    for (size_t i = 0; i < n_total; i++) {
        const mtmd_input_chunk *c = mtmd_input_chunks_get(chunks, i);
        if (mtmd_input_chunk_get_type(c) == MTMD_INPUT_CHUNK_TYPE_IMAGE)
            n_img++;
    }
    if (n_img == 0) {
        mtmd_input_chunks_free(chunks);
        mtmd_bitmap_free(bmp);
        return NULL;
    }

    lagado_patch_result_t *result = (lagado_patch_result_t *)malloc(sizeof(lagado_patch_result_t));
    if (!result) { mtmd_input_chunks_free(chunks); mtmd_bitmap_free(bmp); return NULL; }

    result->n_tiles   = n_img;
    result->refined_w = refined_w;
    result->refined_h = refined_h;
    result->tiles     = (lagado_tile_data_t *)calloc(n_img, sizeof(lagado_tile_data_t));
    if (!result->tiles) {
        free(result);
        mtmd_input_chunks_free(chunks);
        mtmd_bitmap_free(bmp);
        return NULL;
    }

    int img_idx    = 0;       /* index among image chunks only */
    int spatial_i  = 0;       /* index among SPATIAL tiles (excludes overview) */

    for (size_t ci = 0; ci < n_total; ci++) {
        const mtmd_input_chunk *chunk = mtmd_input_chunks_get(chunks, ci);
        if (mtmd_input_chunk_get_type(chunk) != MTMD_INPUT_CHUNK_TYPE_IMAGE)
            continue;

        size_t n_tok = mtmd_input_chunk_get_n_tokens(chunk);

        /* Overview identification: structural position, NOT token count.
         * Spatial tiles occupy indices 0 .. grid_cols*grid_rows - 1.
         * The overview is the trailing chunk at index >= grid_cols*grid_rows.
         * Token count is NOT used here — at some resolutions (e.g. 1025×1025 with a
         * 2×2 grid) the overview can also produce 256 tokens, identical to spatial tiles,
         * making token-count detection wrong. Structure is always correct. */
        int n_spatial = needs_tiling ? (grid_cols * grid_rows) : 0;
        int is_ov     = (img_idx >= n_spatial);

        uint32_t tile_rx = 0, tile_ry = 0;
        if (!is_ov) {
            /* Row-major grid: spatial_i = tile_row * grid_cols + tile_col */
            tile_rx = (uint32_t)((spatial_i % grid_cols) * LFM2_TILE_SIZE);
            tile_ry = (uint32_t)((spatial_i / grid_cols) * LFM2_TILE_SIZE);
            spatial_i++;
        }

        /* Encode chunk */
        float *embd_copy = NULL;
        ret = mtmd_encode_chunk(enc->mtmd_ctx, chunk);
        if (ret == 0) {
            float *raw = mtmd_get_output_embd(enc->mtmd_ctx);
            size_t nbytes = n_tok * (size_t)enc->n_embd * sizeof(float);
            embd_copy = (float *)malloc(nbytes);
            if (embd_copy) memcpy(embd_copy, raw, nbytes);
        }

        result->tiles[img_idx].is_overview    = is_ov;
        result->tiles[img_idx].tile_refined_x = tile_rx;
        result->tiles[img_idx].tile_refined_y = tile_ry;
        result->tiles[img_idx].n_tokens       = (uint32_t)n_tok;
        result->tiles[img_idx].embeddings     = embd_copy;
        img_idx++;
    }

    mtmd_input_chunks_free(chunks);
    mtmd_bitmap_free(bmp);
    return result;
}

void lagado_patch_result_free(lagado_patch_result_t *r)
{
    if (!r) return;
    if (r->tiles) {
        for (size_t i = 0; i < r->n_tiles; i++)
            free(r->tiles[i].embeddings);
        free(r->tiles);
    }
    free(r);
}
