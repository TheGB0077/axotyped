## 0.1.5 (UNRELEASED)

### Features

- collect inner types of `Vec<T>` / `Option<T>` in `#[endpoint]` macro, fixing missing `.ts` files for types that only appear inside containers
- add `group_with()` closure-based grouping with scoped URL prefix, default auth, and TS namespace
- re-export `ts-rs` as `axotyped::TS` so consuming crates don't need a separate `ts-rs` dependency

- forked the crate and rebranded to axotyped
- use forked `ts-rs` dependency to pinned commit with companion-crate support

## 0.1.4 (2026-03-30)

### Fixes

- replace remaining axum-ts-client references with axfetchum
- apply cargo fmt formatting fixes

## 0.1.3 (2026-03-24)

### Features

- switch to crates.io trusted publishing, remove CARGO_REGISTRY_TOKEN

### Fixes

- update crate name references from axum_ts_client to axfetchum
- use dot notation for Authorization header (biome useLiteralKeys)

## 0.1.2 (2026-03-17)

### Features

- switch to crates.io trusted publishing, remove CARGO_REGISTRY_TOKEN

### Fixes

- update crate name references from axum_ts_client to axfetchum

## 0.1.1 (2026-03-17)

### Fixes

- update crate name references from axum_ts_client to axfetchum

## 0.1.7 (2026-03-16)

### Features

- add ApiRouter builder and Vec<T>/Option<T> macro support

## 0.1.6 (2026-03-15)

### Fixes

- resolve fetch at call time for OTel instrumentation compatibility

## 0.1.5 (2026-03-08)

### Fixes

- surface release failures and include refactor commits
- correct knope extra_changelog_sections format

## 0.1.4 (2026-02-14)

### Fixes

- publish crate before git push to prevent cancellation
- add Cargo.lock to versioned_files and --allow-dirty

## 0.1.3 (2026-02-14)

### Features

- add format_command to GeneratorConfig

## 0.1.2 (2026-02-14)

### Fixes

- add CARGO_REGISTRY_GLOBAL_CREDENTIAL_PROVIDERS for cargo publish

## 0.1.1 (2026-02-14)

### Features

- initial axum-ts-client crate
- add automated semver releases via knope + Forgejo CI

### Fixes

- use --strip-components=1 for knope tarball extraction
