use super::InferenceAdapter;
use std::ffi::CString;
use std::os::raw::c_char;

extern "C" {
    fn llama_model_load_from_file(path: *const c_char, params: *const u8) -> *mut u8;
    fn llama_context_new(model: *mut u8, params: *const u8) -> *mut u8;
    fn llama_generate(ctx: *mut u8, tokens: *const i32, n_tokens: i32, n_predict: i32) -> i32;
    fn llama_context_free(ctx: *mut u8);
    fn llama_model_free(model: *mut u8);
}

pub struct LlamaCppAdapter {
    model: *mut u8,
    ctx: *mut u8,
    model_name: String,
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
                model_name: model_path.to_string(),
                context_size,
            })
        }
    }
}

impl InferenceAdapter for LlamaCppAdapter {
    fn generate(&self, _prompt: &str, max_tokens: usize, _temperature: f32) -> Result<String, String> {
        // Full tokenize → generate → detokenize in Phase 1.3
        unsafe {
            let result = llama_generate(self.ctx, std::ptr::null(), 0, max_tokens as i32);
            if result < 0 {
                Err("Generation failed".to_string())
            } else {
                Ok(format!("[generate stub: {} tokens]", result))
            }
        }
    }

    fn supports_kv_slots(&self) -> bool { true }
    fn save_kv_slot(&self, _key: &str) -> Result<(), String> { Ok(()) }
    fn restore_kv_slot(&self, _key: &str) -> Result<bool, String> { Ok(false) }
    fn has_kv_slot(&self, _key: &str) -> bool { false }
    fn model_name(&self) -> &str { &self.model_name }
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
