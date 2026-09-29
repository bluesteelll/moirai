Build cases of the GT20 (b), (d) and (e) lints (docs/m0/PLAN.md WP-02 acceptance). Each directory is a workspace of its
own (an empty `[workspace]` table) that `xtask`'s tests copy into a scratch directory and build there, never in place:
- `cc-build-dep`: a crate with a `cc` build dependency and a build script (rule 1 and rule 2 must refuse it);
- `c-source`: a build script that compiles C source through `cc`; under the gate's poisoned compiler environment the
  build must fail deterministically;
- `clippy-methods`: `f.lock()`, `f.try_lock()`, `f.unlock()` and `path.exists()`, which the syntactic GT20 (d) scan
  cannot tell from `Mutex::lock` or a namesake; clippy refuses them with a product crate's `clippy.toml` (all four),
  with moirai-os's (the three lock calls, not `exists`), and accepts them with no `clippy.toml` (a test-only crate).
