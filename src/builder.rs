//! Axum router builder that collects route metadata alongside real routing.
//!
//! This module provides [`ApiRouter`], a wrapper around [`axum::Router`] that
//! builds both an Axum router and a [`RouteCollection`] from a single definition.
//! Requires the `axum` feature.
//!
//! # Example
//!
//! ```rust,ignore
//! use axotyped::ApiRouter;
//!
//! // Manual type specification (still supported):
//! let (router, routes) = ApiRouter::<AppState>::new()
//!     .get("/users", list_users)
//!         .response::<Vec<UserResponse>>()
//!         .auth()
//!         .done()
//!     .build();
//!
//! // Auto-inferred types via #[endpoint] + register!():
//! let (router, routes) = ApiRouter::<AppState>::new()
//!     .get("/users", register!(list_users))
//!         .auth()
//!         .done()
//!     .build();
//! ```

use axum::Router;
use axum::handler::Handler;
use axum::routing::{self, MethodRouter};

use crate::EndpointMeta;
use crate::types::{HttpMethod, RouteCollection, RouteDefinition};

// ---------------------------------------------------------------------------
// Type collection helper
// ---------------------------------------------------------------------------

/// Trait bound for types that can be collected into the TypeRegistry.
/// When `ts-rs` feature is enabled, this requires `ts_rs::TS`.
/// When disabled, only requires `'static` (collection is a no-op).
#[cfg(feature = "ts-rs")]
pub trait MaybeTs: ts_rs::TS {}

#[cfg(feature = "ts-rs")]
impl<T: ts_rs::TS> MaybeTs for T {}

#[cfg(not(feature = "ts-rs"))]
pub trait MaybeTs: 'static {}

#[cfg(not(feature = "ts-rs"))]
impl<T: 'static> MaybeTs for T {}

/// Register a type for TypeScript export. Deduplicates by TypeId.
/// When the `ts-rs` feature is disabled, this is a no-op.
pub fn collect_type<T: MaybeTs + 'static>(collection: &mut RouteCollection) {
    #[cfg(feature = "ts-rs")]
    collection.register_type::<T>();
    #[cfg(not(feature = "ts-rs"))]
    let _ = collection;
}

// ---------------------------------------------------------------------------
// Metadata sideband (thread-local)
// ---------------------------------------------------------------------------

/// Function pointer type for applying endpoint metadata.
type MetaApplierFn = fn(&mut RouteDefinition, &mut RouteCollection);

thread_local! {
    /// Sideband for passing metadata from `register!()` to the builder methods.
    ///
    /// `register!()` sets this before the handler is passed to `.post()` etc.
    /// The builder method reads and clears it after applying the metadata.
    /// This avoids needing separate method overloads.
    static PENDING_META: std::cell::RefCell<Option<MetaApplierFn>> = const {
        std::cell::RefCell::new(None)
    };
}

/// Set the pending metadata applier. Called by `register!()`.
pub fn set_pending_meta(apply_fn: MetaApplierFn) {
    PENDING_META.with(|m| {
        *m.borrow_mut() = Some(apply_fn);
    });
}

/// Take and clear the pending metadata applier. Called by the builder methods.
fn take_pending_meta() -> Option<MetaApplierFn> {
    PENDING_META.with(|m| m.borrow_mut().take())
}

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

/// Strip module paths from `std::any::type_name` output.
///
/// `"alloc::vec::Vec<myapp::types::UserResponse>"` → `"Vec<UserResponse>"`
fn strip_module_paths(type_name: &str) -> String {
    let mut result = String::with_capacity(type_name.len());
    let mut last_colon_end = 0;
    let bytes = type_name.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b':' && i + 1 < bytes.len() && bytes[i + 1] == b':' {
            last_colon_end = i + 2;
            i += 2;
        } else if bytes[i] == b'<' || bytes[i] == b'>' || bytes[i] == b',' || bytes[i] == b' ' {
            if last_colon_end <= i {
                result.push_str(&type_name[last_colon_end..i]);
            }
            result.push(bytes[i] as char);
            last_colon_end = i + 1;
            i += 1;
        } else {
            i += 1;
        }
    }

    if last_colon_end <= bytes.len() {
        result.push_str(&type_name[last_colon_end..]);
    }

    result
}

/// Get the stripped type name for a Rust type.
pub fn type_string<T: 'static>() -> String {
    strip_module_paths(std::any::type_name::<T>())
}

/// Extract the function name from `std::any::type_name` on a function item.
///
/// `"myapp::plugins::email_password::register"` → `"register"`
fn handler_name_from_type_name(type_name: &str) -> &str {
    // Function type names can have suffixes like `::{{closure}}`, strip those.
    type_name
        .rsplit("::")
        .find(|s| !s.starts_with('{'))
        .unwrap_or(type_name)
}

/// Convert `snake_case` to `camelCase`.
///
/// `"forgot_password"` → `"forgotPassword"`
/// `"list_users"` → `"listUsers"`
/// `"register"` → `"register"` (no-op)
fn snake_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;

    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

/// Derive a camelCase client method name from a handler function's type name.
fn default_name_from_handler<H: 'static>() -> String {
    let full = std::any::type_name::<H>();
    let raw = handler_name_from_type_name(full);
    snake_to_camel(raw)
}

// ---------------------------------------------------------------------------
// ApiRouter
// ---------------------------------------------------------------------------

/// Builder that constructs both an [`axum::Router`] and a [`RouteCollection`].
pub struct ApiRouter<S = ()>
where
    S: Clone + Send + Sync + 'static,
{
    router: Router<S>,
    routes: RouteCollection,
    current_group: Option<String>,
    current_prefix: Option<String>,
    default_auth: bool,
}

impl<S> ApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            routes: RouteCollection::new(),
            current_group: None,
            current_prefix: None,
            default_auth: false,
        }
    }

    /// Set a URL prefix for all subsequent routes.
    ///
    /// Paths registered via `.post()`, `.get()`, etc. will have this prefix
    /// prepended automatically. For example, `.set_prefix("/admin")` followed
    /// by `.post("/course", ...)` registers `/admin/course`.
    pub fn set_prefix(mut self, prefix: &str) -> Self {
        self.current_prefix = Some(prefix.to_string());
        self
    }

    /// Make all subsequent routes require authentication by default.
    ///
    /// The generated TypeScript client will include `auth: true` for every
    /// route, causing the `Authorization: Bearer <token>` header to be sent.
    /// Individual routes can still call `.auth()` (no-op if already set).
    pub fn auth_all(mut self) -> Self {
        self.default_auth = true;
        self
    }

    /// Set the group for all subsequent routes (TS client namespace only).
    ///
    /// For the closure-based version with prefix and auth, see
    /// [`group_with`](Self::group_with).
    pub fn group(mut self, name: &str) -> Self {
        self.current_group = Some(name.to_string());
        self
    }

    /// Clear group, prefix, and default auth (for fluent `group()` usage).
    pub fn no_group(mut self) -> Self {
        self.current_group = None;
        self.current_prefix = None;
        self.default_auth = false;
        self
    }

    /// Closure-based group: scoped prefix, auth, and TS namespace.
    ///
    /// Creates an isolated scope where all routes inherit the group's
    /// prefix, auth setting, and TS client namespace. The group's config
    /// does not leak to routes registered after the closure.
    ///
    /// The prefix defaults to `"/{name}"` but can be overridden with
    /// `.set_prefix()` inside the closure.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// .group_with("admin", |g| {
    ///     g.auth_all()
    ///      .post("/course", create_course)
    ///         .body::<CreateCourseRequest>()
    ///         .response::<CourseRecord>()
    ///         .done()
    ///      .get("/course", list_courses)
    ///         .response::<Vec<CourseRecord>>()
    ///         .done()
    /// })
    /// ```
    pub fn group_with<F>(mut self, name: &str, routes: F) -> Self
    where
        F: FnOnce(ApiRouter<S>) -> ApiRouter<S>,
    {
        let default_prefix = format!("/{}", name);
        let inner = ApiRouter {
            router: Router::new(),
            routes: RouteCollection::new(),
            current_group: Some(name.to_string()),
            current_prefix: Some(default_prefix),
            default_auth: false,
        };

        let inner = routes(inner);

        self.router = self.router.merge(inner.router);
        self.routes.extend(inner.routes);
        self
    }

    /// Merge another `ApiRouter`'s router and routes into this one.
    pub fn merge(mut self, other: ApiRouter<S>) -> Self {
        self.router = self.router.merge(other.router);
        self.routes.extend(other.routes);
        self
    }

    /// Consume the builder and return the router and collected route metadata.
    pub fn build(self) -> (Router<S>, RouteCollection) {
        (self.router, self.routes)
    }

    // --- Standard HTTP method helpers ---

    /// Add a GET route.
    ///
    /// If the handler was wrapped in `register!()`, body/response/query types are
    /// auto-applied from the `#[endpoint]` metadata. Otherwise, use `.body()`,
    /// `.response()`, etc. to specify types manually.
    pub fn get<H, T>(self, path: &str, handler: H) -> RouteBuilder<S>
    where
        H: Handler<T, S> + 'static,
        T: 'static,
    {
        let name = default_name_from_handler::<H>();
        let mut builder = self.route(path, HttpMethod::Get, routing::get(handler), name);
        builder.apply_pending_meta();
        builder
    }

    /// Add a POST route.
    pub fn post<H, T>(self, path: &str, handler: H) -> RouteBuilder<S>
    where
        H: Handler<T, S> + 'static,
        T: 'static,
    {
        let name = default_name_from_handler::<H>();
        let mut builder = self.route(path, HttpMethod::Post, routing::post(handler), name);
        builder.apply_pending_meta();
        builder
    }

    /// Add a PUT route.
    pub fn put<H, T>(self, path: &str, handler: H) -> RouteBuilder<S>
    where
        H: Handler<T, S> + 'static,
        T: 'static,
    {
        let name = default_name_from_handler::<H>();
        let mut builder = self.route(path, HttpMethod::Put, routing::put(handler), name);
        builder.apply_pending_meta();
        builder
    }

    /// Add a PATCH route.
    pub fn patch<H, T>(self, path: &str, handler: H) -> RouteBuilder<S>
    where
        H: Handler<T, S> + 'static,
        T: 'static,
    {
        let name = default_name_from_handler::<H>();
        let mut builder = self.route(path, HttpMethod::Patch, routing::patch(handler), name);
        builder.apply_pending_meta();
        builder
    }

    /// Add a DELETE route.
    pub fn delete<H, T>(self, path: &str, handler: H) -> RouteBuilder<S>
    where
        H: Handler<T, S> + 'static,
        T: 'static,
    {
        let name = default_name_from_handler::<H>();
        let mut builder = self.route(path, HttpMethod::Delete, routing::delete(handler), name);
        builder.apply_pending_meta();
        builder
    }

    /// Add a WebSocket route.
    ///
    /// Returns a [`WsRouteBuilder`] that only exposes WS-relevant methods
    /// (`.query()`, `.events()`, `.auth()`, `.done()`). Internally uses
    /// `routing::get()` since WebSocket upgrades start as HTTP GET requests.
    pub fn ws<H, T>(mut self, path: &str, handler: H) -> WsRouteBuilder<S>
    where
        H: Handler<T, S> + 'static,
        T: 'static,
    {
        let name = default_name_from_handler::<H>();
        let full_path = self.resolve_path(path);
        self.router = self.router.route(&full_path, routing::get(handler));
        let def = RouteDefinition {
            name,
            method: HttpMethod::Get,
            path: full_path,
            auth: self.default_auth,
            body_type: None,
            response_type: None,
            query_type: None,
            path_params: crate::extract_path_params(path),
            group: self.current_group.clone(),
            redirect: false,
            websocket: true,
            ws_send_type: None,
            ws_receive_type: None,
        };

        WsRouteBuilder { parent: self, def }
    }

    fn resolve_path(&self, path: &str) -> String {
        match &self.current_prefix {
            Some(prefix) => format!("{}{}", prefix, path),
            None => path.to_string(),
        }
    }

    fn route(
        mut self,
        path: &str,
        method: HttpMethod,
        method_router: MethodRouter<S>,
        default_name: String,
    ) -> RouteBuilder<S> {
        let full_path = self.resolve_path(path);
        self.router = self.router.route(&full_path, method_router);
        let def = RouteDefinition {
            name: default_name,
            method,
            path: full_path,
            auth: self.default_auth,
            body_type: None,
            response_type: None,
            query_type: None,
            path_params: crate::extract_path_params(path),
            group: self.current_group.clone(),
            redirect: false,
            websocket: false,
            ws_send_type: None,
            ws_receive_type: None,
        };
        RouteBuilder { parent: self, def }
    }
}

impl<S> Default for ApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// RouteBuilder
// ---------------------------------------------------------------------------

/// In-progress route definition. Chain `.body::<T>()`, `.response::<T>()`,
/// `.auth()`, `.redirect()`, then finalize with `.done()` or `.as_("name")`.
pub struct RouteBuilder<S>
where
    S: Clone + Send + Sync + 'static,
{
    parent: ApiRouter<S>,
    def: RouteDefinition,
}

impl<S> RouteBuilder<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Apply any pending metadata from `register!()`. Called internally by the
    /// HTTP method helpers right after route registration.
    fn apply_pending_meta(&mut self) {
        if let Some(apply_fn) = take_pending_meta() {
            apply_fn(&mut self.def, &mut self.parent.routes);
        }
    }

    /// Set the request body type.
    pub fn body<T: MaybeTs + 'static>(mut self) -> Self {
        self.def.body_type = Some(type_string::<T>());
        collect_type::<T>(&mut self.parent.routes);
        self
    }

    /// Set the response type.
    pub fn response<T: MaybeTs + 'static>(mut self) -> Self {
        self.def.response_type = Some(type_string::<T>());
        collect_type::<T>(&mut self.parent.routes);
        self
    }

    /// Set the query parameters type.
    pub fn query<T: MaybeTs + 'static>(mut self) -> Self {
        self.def.query_type = Some(type_string::<T>());
        collect_type::<T>(&mut self.parent.routes);
        self
    }

    /// Set both body and response types at once.
    ///
    /// ```rust,ignore
    /// .post("/users", create_user)
    ///     .json::<CreateUserRequest, UserResponse>()
    ///     .auth()
    ///     .done()
    /// ```
    pub fn json<B: MaybeTs + 'static, R: MaybeTs + 'static>(mut self) -> Self {
        self.def.body_type = Some(type_string::<B>());
        self.def.response_type = Some(type_string::<R>());
        collect_type::<B>(&mut self.parent.routes);
        collect_type::<R>(&mut self.parent.routes);
        self
    }

    /// Mark this route as requiring authentication.
    pub fn auth(mut self) -> Self {
        self.def.auth = true;
        self
    }

    /// Mark this route as a browser redirect (URL builder, not fetch).
    pub fn redirect(mut self) -> Self {
        self.def.redirect = true;
        self
    }

    /// Finalize the route using the auto-derived name (handler function name → camelCase).
    pub fn done(mut self) -> ApiRouter<S> {
        // name was already set from the handler in ApiRouter::route()
        self.parent.routes.push(self.def);
        self.parent
    }

    /// Finalize the route with an explicit client method name, overriding the auto-derived name.
    pub fn as_(mut self, name: &str) -> ApiRouter<S> {
        self.def.name = name.to_string();
        self.parent.routes.push(self.def);
        self.parent
    }
}

// ---------------------------------------------------------------------------
// WsRouteBuilder
// ---------------------------------------------------------------------------

/// Constrained builder for WebSocket routes.
///
/// Only exposes methods relevant to WebSocket endpoints:
/// - `.query::<T>()` — query parameters (e.g., auth token)
/// - `.events::<S, R>()` — client-to-server (`S`) and server-to-client (`R`) event types
/// - `.auth()` — mark as requiring authentication
/// - `.done()` / `.as_("name")` — finalize
///
/// Methods that don't make sense for WS (`.body()`, `.response()`, `.json()`,
/// `.redirect()`) are not available.
pub struct WsRouteBuilder<S>
where
    S: Clone + Send + Sync + 'static,
{
    parent: ApiRouter<S>,
    def: RouteDefinition,
}

impl<S> WsRouteBuilder<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Set the query parameters type.
    pub fn query<T: MaybeTs + 'static>(mut self) -> Self {
        self.def.query_type = Some(type_string::<T>());
        collect_type::<T>(&mut self.parent.routes);
        self
    }

    /// Set the event types for this WebSocket endpoint.
    ///
    /// - `S`: client-to-server event type (what the TS client sends)
    /// - `R`: server-to-client event type (what the TS client receives)
    ///
    /// Generates a TypeScript `TypedWebSocket<S, R>` wrapper with typed
    /// `send(event: S)` and `onMessage(handler: (event: R) => void)` methods.
    pub fn events<Send: MaybeTs + 'static, Receive: MaybeTs + 'static>(mut self) -> Self {
        self.def.ws_send_type = Some(type_string::<Send>());
        self.def.ws_receive_type = Some(type_string::<Receive>());
        collect_type::<Send>(&mut self.parent.routes);
        collect_type::<Receive>(&mut self.parent.routes);
        self
    }

    /// Mark this route as requiring authentication.
    pub fn auth(mut self) -> Self {
        self.def.auth = true;
        self
    }

    /// Finalize the route using the auto-derived name (handler function name → camelCase).
    pub fn done(mut self) -> ApiRouter<S> {
        self.parent.routes.push(self.def);
        self.parent
    }

    /// Finalize the route with an explicit client method name, overriding the auto-derived name.
    pub fn as_(mut self, name: &str) -> ApiRouter<S> {
        self.def.name = name.to_string();
        self.parent.routes.push(self.def);
        self.parent
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- strip_module_paths --

    #[test]
    fn strip_simple_type() {
        assert_eq!(
            strip_module_paths("myapp::types::UserResponse"),
            "UserResponse"
        );
    }

    #[test]
    fn strip_vec_generic() {
        assert_eq!(
            strip_module_paths("alloc::vec::Vec<myapp::types::UserResponse>"),
            "Vec<UserResponse>"
        );
    }

    #[test]
    fn strip_option_generic() {
        assert_eq!(
            strip_module_paths("core::option::Option<myapp::types::UserResponse>"),
            "Option<UserResponse>"
        );
    }

    #[test]
    fn strip_plain_type() {
        assert_eq!(strip_module_paths("String"), "String");
    }

    #[test]
    fn strip_nested_generic() {
        assert_eq!(
            strip_module_paths("alloc::vec::Vec<core::option::Option<myapp::Foo>>"),
            "Vec<Option<Foo>>"
        );
    }

    // -- type_string --

    #[test]
    fn type_string_for_vec() {
        assert_eq!(type_string::<Vec<String>>(), "Vec<String>");
    }

    #[test]
    fn type_string_for_option() {
        assert_eq!(type_string::<Option<String>>(), "Option<String>");
    }

    #[test]
    fn type_string_for_plain() {
        assert_eq!(type_string::<String>(), "String");
    }

    // -- snake_to_camel --

    #[test]
    fn camel_simple() {
        assert_eq!(snake_to_camel("register"), "register");
    }

    #[test]
    fn camel_two_words() {
        assert_eq!(snake_to_camel("forgot_password"), "forgotPassword");
    }

    #[test]
    fn camel_three_words() {
        assert_eq!(snake_to_camel("list_all_users"), "listAllUsers");
    }

    #[test]
    fn camel_already_camel() {
        assert_eq!(snake_to_camel("listUsers"), "listUsers");
    }

    // -- handler_name_from_type_name --

    #[test]
    fn handler_name_simple() {
        assert_eq!(
            handler_name_from_type_name("myapp::plugins::email_password::register"),
            "register"
        );
    }

    #[test]
    fn handler_name_nested() {
        assert_eq!(
            handler_name_from_type_name("myapp::handlers::admin::list_users"),
            "list_users"
        );
    }

    #[test]
    fn handler_name_closure() {
        assert_eq!(
            handler_name_from_type_name("myapp::routes::handler::{{closure}}"),
            "handler"
        );
    }

    // -- default_name_from_handler --

    fn dummy_list_users() {}
    fn dummy_forgot_password() {}
    fn dummy_register() {}

    #[test]
    fn default_name_list_users() {
        let name = default_name_from_handler::<fn()>();
        // For a plain fn() type, type_name is just the type signature, not useful.
        // The real test is with named function items in builder_tests.rs.
        // Here we just test the helpers individually.
        assert!(!name.is_empty());
    }

    #[test]
    fn default_name_via_snake_to_camel() {
        // Simulate what happens: handler type_name ends with "list_users"
        let raw = handler_name_from_type_name("myapp::handlers::list_users");
        let name = snake_to_camel(raw);
        assert_eq!(name, "listUsers");
    }

    #[test]
    fn default_name_no_underscore() {
        let raw = handler_name_from_type_name("myapp::handlers::register");
        let name = snake_to_camel(raw);
        assert_eq!(name, "register");
    }
}
