# Vendored OpenSSL for musl release builds

## Symptoms

- GitHub Actions `release` workflow passes `rustfmt`, `clippy`, and `test`, then fails in `build assets (linux musl)`.
- `cross build --target x86_64-unknown-linux-musl` aborts inside `openssl-sys`.
- The failure complains that `openssl.pc` or a usable system OpenSSL installation cannot be found in the musl build container.
- No release binaries are produced, so the GitHub Release and follow-up deployment remain blocked.

## Root cause

The VLESS HTTPS canary introduced a direct `openssl` crate dependency for certificate parsing. The release workflow builds Linux musl binaries inside `cross` containers, and those containers do not guarantee a system OpenSSL development package with a discoverable `openssl.pc`.

That means the new dependency works for ordinary host builds but breaks the release packaging path, because `openssl-sys` tries to locate a system OpenSSL during musl cross-compilation.

## Fix used here

Enable the official `vendored` feature on the Rust `openssl` crate:

```toml
openssl = { version = "0.10", features = ["vendored"] }
```

This pulls in `openssl-src`, so release builds compile and statically link OpenSSL instead of depending on a system package inside the `cross` image.

Keep the fix at the dependency layer rather than patching the release workflow image, because:

- the release workflow already uses the standard `cross` musl images
- the blocker is specific to the new runtime dependency, not to the release orchestration
- vendoring is the crate's documented path for environments without a preinstalled OpenSSL toolchain

## Verification

- `Cargo.lock` now includes `openssl-src`.
- `cargo check --locked --bin xp-ops` still succeeds after the dependency change.
- Re-run the `release` workflow and confirm `build assets (linux musl)` no longer fails on `openssl-sys`.
- Confirm a new release tag produces `xp-linux-x86_64`, `xp-ops-linux-x86_64`, and the rest of the expected assets.
