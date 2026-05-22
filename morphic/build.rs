//! `intel_tex_2` ships prebuilt ISPC object files (incl. the ASTC kernel,
//! which we don't use but which is unconditionally linked). Those object
//! files have a `.cxx_eh` dependency on the C++ runtime's
//! `__gxx_personality_v0`. The crate itself doesn't declare that link, so
//! we add it here. Without this, linking on Linux fails with
//! `undefined symbol: __gxx_personality_v0`.

fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target.as_str() {
        "linux" | "android" => println!("cargo:rustc-link-lib=stdc++"),
        "macos" | "ios" => println!("cargo:rustc-link-lib=c++"),
        // MSVC links the C++ runtime by default; nothing to do.
        _ => {}
    }
}
