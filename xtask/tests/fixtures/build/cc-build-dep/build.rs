//! Seeded build case: a build script with a `cc` build dependency that compiles nothing.

fn main() {
    let _ = cc::Build::new();
}
