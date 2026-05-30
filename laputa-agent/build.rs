fn main() {
    println!("cargo:rustc-link-search=native=vendored/llama.cpp-2");
    println!("cargo:rustc-link-lib=dylib=llama");
    println!("cargo:rustc-link-lib=dylib=ggml-base");
    println!("cargo:rustc-link-lib=dylib=ggml-cpu");
    println!("cargo:rustc-link-lib=dylib=ggml-cuda");
}
