//! Seeded build case: compiles `native.c` through the `cc` crate.

fn main() {
    cc::Build::new().file("native.c").compile("seeded_native");
}
