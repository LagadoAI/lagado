fn main() {
    // Link against vendored llama.cpp shared libs
    println!("cargo:rustc-link-search=native=vendored/llama.cpp-2");
    println!("cargo:rustc-link-lib=dylib=llama");
    println!("cargo:rustc-link-lib=dylib=ggml");
    println!("cargo:rustc-link-lib=dylib=ggml-cuda");
    
    // Add vendored lib path to runtime library search
    println!("cargo:rustc-env=LD_LIBRARY_PATH=vendored/llama.cpp-2");
}
