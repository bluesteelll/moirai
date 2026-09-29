//! Build script of the fuzz package: fallback A of docs/m0/tools.md §4.4, failure 1.
//!
//! On `x86_64-pc-windows-msvc` with the sanitizer off, a fuzz target does not link: the SanitizerCoverage section
//! bounds (`__start___sancov_cntrs` and the rest) are defined only by MSVC's ASan runtime. This script compiles
//! `src/sancov_sections.c`, which defines them, with the `cc` crate (already in this graph through libfuzzer-sys) and
//! passes its object to the linker of every linked target with `cargo::rustc-link-arg`: `rustc-link-arg-bins` is
//! refused while the package has no binary target (before FL-1's first fuzz target), and a `rustc-link-lib` would
//! reach only the targets that use this package's library. In a target built without SanitizerCoverage (the
//! library's unit tests) the object defines eight unused symbols. Nothing is compiled for other targets, where the
//! linker synthesises the bounds, nor under a sanitizer (`cargo fuzz -s address`), whose runtime defines them itself
//! and would collide with a second definition.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/sancov_sections.c");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc");
    let sanitizer = std::env::var_os("CARGO_CFG_SANITIZE").is_some();
    if !msvc || sanitizer {
        return;
    }
    let objects = cc::Build::new()
        .file("src/sancov_sections.c")
        .cargo_metadata(false)
        .compile_intermediates();
    for o in objects {
        println!("cargo::rustc-link-arg={}", o.display());
    }
}
