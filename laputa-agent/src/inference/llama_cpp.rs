use super::InferenceAdapter;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};

type LlamaToken = i32;

extern "C" {
    fn llama_model_load_from_file(path: *const c_char, params: *const u8) -> *mut u8;
    fn llama_context_new(model: *mut u8, params: *const u8) -> *mut u8;
    fn llama_context_free(ctx: *mut u8);
    fn llama_model_free(model: *mut u8);
    fn llama_model_get_vocab(model: *const u8) -> *const u8;

    fn llama_tokenize(
        vocab: *const u8,
        text: *const c_char,
        text_len: i32,
        tokens: *mut LlamaToken,
        n_tokens_max: i32,
        add_special: bool,
        parse_special: bool,
    ) -> i32;

    fn llama_token_to_piece(
        vocab: *const u8,
        token: LlamaToken,
        buf: *mut c_char,
        length: i32,
        lstrip: i32,
        special: bool,
    ) -> i32;

    fn llama_decode(ctx: *mut u8, batch: LlamaBatch) -> i32;
    fn llama_batch_get_one(tokens: *mut LlamaToken, n_tokens: i32) -> LlamaBatch;
    fn llama_sampler_chain_default_params() -> *mut u8;
    fn llama_sampler_chain_init(params: *mut u8) -> *mut u8;
    fn llama_sampler_chain_add(chain: *mut u8, smpl: *mut u8);
    fn llama_sampler_init_greedy() -> *mut u8;
    fn llama_sampler_sample(smpl: *mut u8, ctx: *mut u8, idx: i32) -> LlamaToken;
    fn llama_sampler_free(smpl: *mut u8);
    fn llama_token_is_eog(vocab: *const u8, token: LlamaToken) -> bool;
    fn llama_model_n_ctx_train(model: *const u8) -> i32;
}

#[repr(C)]
struct LlamaBatch {
    n_tokens: i32,
    token: *mut LlamaToken,
    embd: *mut f32,
    pos: *mut i32,
    n_seq_id: *mut i32,
    seq_id: *mut *mut i32,
    logits: *mut i8,
}

pub struct LlamaCppAdapter {
    model: *mut u8,
    ctx: *mut u8,
    model_path: String,
    context_size: usize,
}

unsafe impl Send for LlamaCppAdapter {}
unsafe impl Sync for LlamaCppAdapter {}

impl LlamaCppAdapter {
    pub fn new(model_path: &str, context_size: usize) -> Result<Self, String> {
        let path = CString::new(model_path)
            .map_err(|e| format!("Invalid model path: {}", e))?;
        unsafe {
            let model = llama_model_load_from_file(path.as_ptr(), std::ptr::null());
            if model.is_null() {
                return Err(format!("Failed to load model: {}", model_path));
            }
            let ctx = llama_context_new(model, std::ptr::null());
            if ctx.is_null() {
                llama_model_free(model);
                return Err("Failed to create context".to_string());
            }
            Ok(LlamaCppAdapter {
                model,
                ctx,
                model_path: model_path.to_string(),
                context_size,
            })
        }
    }

    fn tokenize(&self, text: &str) -> Result<Vec<LlamaToken>, String> {
        let ctext = CString::new(text).map_err(|e| e.to_string())?;
        let max = text.len() as i32 + 16;
        let mut tokens = vec![0i32; max as usize];
        unsafe {
            let vocab = llama_model_get_vocab(self.model);
            let n = llama_tokenize(
                vocab,
                ctext.as_ptr(),
                text.len() as i32,
                tokens.as_mut_ptr(),
                max,
                true,
                true,
            );
            if n < 0 {
                return Err(format!("Tokenize failed: {}", n));
            }
            tokens.truncate(n as usize);
        }
        Ok(tokens)
    }

    fn token_to_string(&self, token: LlamaToken) -> String {
        let mut buf = vec![0u8; 32];
        unsafe {
            let vocab = llama_model_get_vocab(self.model);
            let n = llama_token_to_piece(
                vocab,
                token,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as i32,
                0,
                false,
            );
            if n > 0 {
                String::from_utf8_lossy(&buf[..n as usize]).to_string()
            } else {
                String::new()
            }
        }
    }
}

impl InferenceAdapter for LlamaCppAdapter {
    fn generate(&self, prompt: &str, max_tokens: usize, _temperature: f32) -> Result<String, String> {
        let tokens = self.tokenize(prompt)?;
        let mut token_list = tokens.clone();
        let mut output = String::new();

        unsafe {
            let vocab = llama_model_get_vocab(self.model);

            // Decode the prompt
            let batch = llama_batch_get_one(token_list.as_mut_ptr(), token_list.len() as i32);
            let ret = llama_decode(self.ctx, batch);
            if ret != 0 {
                return Err(format!("llama_decode failed: {}", ret));
            }

            // Set up greedy sampler
            let sparams = llama_sampler_chain_default_params();
            let sampler = llama_sampler_chain_init(sparams);
            llama_sampler_chain_add(sampler, llama_sampler_init_greedy());

            // Generate tokens
            for _ in 0..max_tokens {
                let token = llama_sampler_sample(sampler, self.ctx, -1);
                if llama_token_is_eog(vocab, token) {
                    break;
                }
                output.push_str(&self.token_to_string(token));

                let mut next = vec![token];
                let next_batch = llama_batch_get_one(next.as_mut_ptr(), 1);
                let ret = llama_decode(self.ctx, next_batch);
                if ret != 0 {
                    break;
                }
            }

            llama_sampler_free(sampler);
        }
        Ok(output)
    }

    fn supports_kv_slots(&self) -> bool { true }
    fn save_kv_slot(&self, _key: &str) -> Result<(), String> { Ok(()) }
    fn restore_kv_slot(&self, _key: &str) -> Result<bool, String> { Ok(false) }
    fn has_kv_slot(&self, _key: &str) -> bool { false }
    fn model_name(&self) -> &str { &self.model_path }
    fn context_size(&self) -> usize { self.context_size }
}

impl Drop for LlamaCppAdapter {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() { llama_context_free(self.ctx); }
            if !self.model.is_null() { llama_model_free(self.model); }
        }
    }
}
