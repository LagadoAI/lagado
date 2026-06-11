fn main() {
    // Visual encoder shim — Linux only, and only when vendored headers are present.
    // Sets cargo:rustc-cfg=lagado_vision_ffi so vision/mod.rs gates FFI accordingly.
    #[cfg(target_os = "linux")]
    build_vision_shim();
}

#[cfg(target_os = "linux")]
fn build_vision_shim() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let include_dir = format!("{manifest}/vendored/llama.cpp-2/include");

    // Skip gracefully when vendored headers are absent (e.g. CI without local build artifacts).
    if !std::path::Path::new(&include_dir).join("llama.h").exists() {
        println!("cargo:warning=vision shim skipped: vendored/llama.cpp-2/include/llama.h not found");
        return;
    }

    let lib_dir = format!("{manifest}/vendored/llama.cpp-2/build/bin");
    let mtmd_include_dir = format!("{manifest}/vendored/llama.cpp-2/tools/mtmd");
    let ggml_include_dir = format!("{manifest}/vendored/llama.cpp-2/ggml/include");

    println!("cargo:rerun-if-changed=src/vision/shim.c");
    println!("cargo:rerun-if-changed=vendored/llama.cpp-2/include/llama.h");
    println!("cargo:rerun-if-changed=vendored/llama.cpp-2/tools/mtmd/mtmd.h");

    cc::Build::new()
        .file("src/vision/shim.c")
        .include(&include_dir)
        .include(&mtmd_include_dir)
        .include(&ggml_include_dir)
        .flag("-w")
        .compile("lagado_vision_shim");

    println!("cargo:rustc-link-search=native={lib_dir}");
    println!("cargo:rustc-link-lib=dylib=llama");
    println!("cargo:rustc-link-lib=dylib=mtmd");
    println!("cargo:rustc-link-lib=dylib=ggml");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_dir}");
    println!("cargo:rustc-cfg=lagado_vision_ffi");
}
