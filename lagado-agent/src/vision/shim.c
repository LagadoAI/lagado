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
