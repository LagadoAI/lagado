fn main() {
    // Visual encoder shim — Linux only (libmtmd.so is a Linux-only build artifact)
    #[cfg(target_os = "linux")]
    build_vision_shim();
}

#[cfg(target_os = "linux")]
fn build_vision_shim() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let lib_dir = format!("{manifest}/vendored/llama.cpp-2/build/bin");
    let include_dir = format!("{manifest}/vendored/llama.cpp-2/include");
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
        .flag("-w") // suppress warnings from llama.cpp headers
        .compile("lagado_vision_shim");

    println!("cargo:rustc-link-search=native={lib_dir}");
    println!("cargo:rustc-link-lib=dylib=llama");
    println!("cargo:rustc-link-lib=dylib=mtmd");
    println!("cargo:rustc-link-lib=dylib=ggml");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_dir}");
}
