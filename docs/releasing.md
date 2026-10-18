# Release checklist

This is a maintainer checklist, not part of the published crate. A prerelease
is a field-test build: it may receive API changes and does not make the 1.0
compatibility or production-readiness promise.

Publishing, tagging, and creating a hosted release require explicit maintainer
approval. Preparation work must not perform them implicitly.

## Prepare

1. Choose the version and update `Cargo.toml`, `Cargo.lock`, the README
   dependency snippets, and `CHANGELOG.md` together.
2. Replace `Unreleased` in the changelog with the publication date.
3. Confirm the worktree is clean and the intended release commit is on `main`.
4. Inspect `cargo package --list`; contributor protocols, generated output,
   secrets, and unrelated research must not be present.
5. Run the current lightweight UX pass when public API, timing, error, or
   documentation behavior materially changed.

## Verify

Run the project quality gates:

```text
cargo fmt --all -- --check
cargo test --all-features
cargo check --no-default-features --features libm
cargo test --no-default-features --features libm --lib
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps
cargo package
cargo publish --dry-run
```

Also build two fresh scratch crates against the packaged source: the standard
README quick start and a `#![no_std]` library using `f32` with the `libm`
feature. Run a representative response plot and ordinary streaming benchmark;
record regressions or surprising resource use, not host-specific timing as a
portable promise.

## Publish after approval

1. Run `cargo publish` from the clean, verified release commit.
2. After publication succeeds, create tag `v<version>` at that exact commit and
   push the tag.
3. Create the hosted release from the matching changelog section.
4. Verify the crates.io metadata, docs.rs build, license files, README links,
   and a new dependency resolution using the registry version.
5. Keep the prerelease label until field feedback supports a stable release.

The project intentionally follows current stable Rust during pre-1.0
development. Adding an MSRV or changing that policy is a separate public
compatibility decision.
