//! # axfetchum
//!
//! Auto-generate typed TypeScript API clients from Axum route metadata.
//!
//! This crate provides:
//! - A declarative `api_routes!` macro for defining route metadata
//! - A TypeScript client code generator that produces typed fetch wrappers
//! - A `check()` function for CI that fails if the generated client is stale
//!
//! ## Zero Dependencies
//!
//! This crate has no external dependencies — it uses only `std`.
//! Type generation (the actual `.ts` type files) is handled by [`ts-rs`](https://crates.io/crates/ts-rs)
//! in the consuming crate.
//!
//! ## Quick Start
//!
//! ```rust
//! use axfetchum::{api_routes, RouteCollection};
//!
//! fn my_routes() -> RouteCollection {
//!     api_routes! {
//!         @group users
//!
//!         list: GET "/users" [auth]
//!             -> UsersResponse;
//!         getById: GET "/users/{id}" [auth]
//!             -> UserResponse;
//!         create: POST "/users" [auth]
//!             body: CreateUserRequest -> UserResponse;
//!     }
//! }
//! ```

mod generator;
#[macro_use]
mod macros;
mod types;

#[cfg(feature = "axum")]
mod builder;

// Re-export public API
pub use generator::{CheckError, GeneratorConfig, check, generate, generate_to_file};
pub use types::{HttpMethod, PathParam, RouteCollection, RouteDefinition, TypeRegistry, extract_path_params};

#[cfg(feature = "axum")]
pub use builder::{ApiRouter, MaybeTs, RouteBuilder, WsRouteBuilder};
