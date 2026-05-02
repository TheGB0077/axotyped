/// HTTP method for a route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A path parameter extracted from a route path (e.g., `{id}` in `/users/{id}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathParam {
    pub name: String,
}

/// Definition of a single API route.
#[derive(Debug, Clone)]
pub struct RouteDefinition {
    /// Function name in the generated client (e.g., `register`, `listUsers`).
    pub name: String,
    /// HTTP method.
    pub method: HttpMethod,
    /// Route path (e.g., `/register`, `/admin/users/{id}`).
    pub path: String,
    /// Whether the route requires authentication.
    pub auth: bool,
    /// Rust type name of the request body (stringified via `stringify!()`).
    pub body_type: Option<String>,
    /// Rust type name of the response body (stringified via `stringify!()`).
    pub response_type: Option<String>,
    /// Rust type name for query parameters (stringified via `stringify!()`).
    pub query_type: Option<String>,
    /// Path parameters extracted from the path.
    pub path_params: Vec<PathParam>,
    /// Group name for nested object structure (e.g., `emailPassword`).
    pub group: Option<String>,
    /// Whether this route is a browser redirect (not a fetch call).
    pub redirect: bool,
    /// Whether this route is a WebSocket endpoint (generates WS connection, not fetch).
    pub websocket: bool,
    /// Rust type name for client-to-server events (send direction).
    pub ws_send_type: Option<String>,
    /// Rust type name for server-to-client events (receive direction).
    pub ws_receive_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Type collection (ts-rs feature)
// ---------------------------------------------------------------------------

/// A type-erased `T::export_all(&cfg)` function pointer.
/// Used to call ts-rs's export mechanism for types discovered during route building.
#[cfg(feature = "ts-rs")]
type ExportFn = fn(&ts_rs::Config) -> Result<(), ts_rs::ExportError>;

/// Collects types encountered during route building so their TypeScript
/// declarations can be exported via ts-rs's `export_all()` mechanism.
/// Deduplicates by `TypeId` to handle generic instantiations correctly
/// (e.g., `ContentResponse<VocabItem>` and `ContentResponse<Dialog>` share
/// the same generic `TS` impl and produce the same declaration).
#[cfg(feature = "ts-rs")]
#[derive(Debug, Clone, Default)]
pub struct TypeRegistry {
    /// TypeId → export_all function pointer, for deduplication and export.
    slots: Vec<(std::any::TypeId, ExportFn)>,
    seen: std::collections::BTreeSet<std::any::TypeId>,
}

#[cfg(feature = "ts-rs")]
impl TypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a type's export function. Deduplicates by `TypeId`.
    pub fn register<T: ts_rs::TS + 'static>(&mut self) {
        let type_id = std::any::TypeId::of::<T>();
        if self.seen.insert(type_id) {
            self.slots.push((type_id, T::export_all));
        }
    }

    /// Whether a type has already been registered.
    pub fn contains_type<T: 'static>(&self) -> bool {
        self.seen.contains(&std::any::TypeId::of::<T>())
    }

    /// All registered export function pointers.
    pub fn slots(&self) -> &[(std::any::TypeId, ExportFn)] {
        &self.slots
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Merge another registry into this one. Duplicates are skipped.
    pub fn extend(&mut self, other: TypeRegistry) {
        for (type_id, export_fn) in other.slots {
            if self.seen.insert(type_id) {
                self.slots.push((type_id, export_fn));
            }
        }
    }
}

/// Placeholder registry when ts-rs is not enabled — no type collection.
#[cfg(not(feature = "ts-rs"))]
#[derive(Debug, Clone, Default)]
pub struct TypeRegistry;

#[cfg(not(feature = "ts-rs"))]
impl TypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        true
    }

    pub fn len(&self) -> usize {
        0
    }

    pub fn extend(&mut self, _other: TypeRegistry) {}
}

/// A collection of route definitions and their associated TypeScript types.
#[derive(Debug, Clone, Default)]
pub struct RouteCollection {
    routes: Vec<RouteDefinition>,
    types: TypeRegistry,
}

impl RouteCollection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, route: RouteDefinition) {
        self.routes.push(route);
    }

    pub fn extend(&mut self, other: RouteCollection) {
        self.routes.extend(other.routes);
        self.types.extend(other.types);
    }

    pub fn routes(&self) -> &[RouteDefinition] {
        &self.routes
    }

    pub fn types(&self) -> &TypeRegistry {
        &self.types
    }

    /// Register a type for TypeScript export. Deduplicates by TypeId.
    #[cfg(feature = "ts-rs")]
    pub fn register_type<T: ts_rs::TS + 'static>(&mut self) {
        self.types.register::<T>();
    }

    /// Export all collected types (and their transitive dependencies) to the
    /// given directory using ts-rs's `export_all()` mechanism.
    ///
    /// Call this before `generate_to_file()` — the generated client will
    /// import types from this directory.
    ///
    /// This replaces manual `T::export_all(&cfg)` calls for each type.
    /// Types are auto-discovered from the builder's `.response::<T>()`,
    /// `.body::<T>()`, `.query::<T>()`, and `.events::<A, B>()` calls.
    #[cfg(feature = "ts-rs")]
    pub fn export_types(&self, dir: &std::path::Path) -> Result<(), std::io::Error> {
        use std::fs;

        if self.types.is_empty() {
            return Ok(());
        }

        fs::create_dir_all(dir)?;
        let cfg = ts_rs::Config::new().with_out_dir(dir.to_path_buf());

        for (_, export_fn) in self.types.slots() {
            if let Err(e) = export_fn(&cfg) {
                // Log but don't fail — one bad type shouldn't block generation
                eprintln!("axfetchum: type export failed: {e}");
            }
        }

        Ok(())
    }

    pub fn len(&self) -> usize {
        self.routes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, RouteDefinition> {
        self.routes.iter()
    }
}

impl IntoIterator for RouteCollection {
    type Item = RouteDefinition;
    type IntoIter = std::vec::IntoIter<RouteDefinition>;

    fn into_iter(self) -> Self::IntoIter {
        self.routes.into_iter()
    }
}

impl<'a> IntoIterator for &'a RouteCollection {
    type Item = &'a RouteDefinition;
    type IntoIter = std::slice::Iter<'a, RouteDefinition>;

    fn into_iter(self) -> Self::IntoIter {
        self.routes.iter()
    }
}

/// Extract path parameters from a route path string.
///
/// For example, `/admin/users/{id}` returns `[PathParam { name: "id" }]`.
pub fn extract_path_params(path: &str) -> Vec<PathParam> {
    path.split('/')
        .filter_map(|seg| {
            seg.strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .map(|name| PathParam {
                    name: name.to_string(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_no_params() {
        assert!(extract_path_params("/register").is_empty());
        assert!(extract_path_params("/admin/users").is_empty());
    }

    #[test]
    fn extract_single_param() {
        let params = extract_path_params("/admin/users/{id}");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "id");
    }

    #[test]
    fn extract_multiple_params() {
        let params = extract_path_params("/orgs/{org_id}/users/{user_id}");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "org_id");
        assert_eq!(params[1].name, "user_id");
    }

    #[test]
    fn route_collection_extend() {
        let mut a = RouteCollection::new();
        a.push(RouteDefinition {
            name: "foo".into(),
            method: HttpMethod::Get,
            path: "/foo".into(),
            auth: false,
            body_type: None,
            response_type: None,
            query_type: None,
            path_params: vec![],
            group: None,
            redirect: false,
            websocket: false,
            ws_send_type: None,
            ws_receive_type: None,
        });

        let mut b = RouteCollection::new();
        b.push(RouteDefinition {
            name: "bar".into(),
            method: HttpMethod::Post,
            path: "/bar".into(),
            auth: true,
            body_type: Some("BarRequest".into()),
            response_type: Some("BarResponse".into()),
            query_type: None,
            path_params: vec![],
            group: Some("baz".into()),
            redirect: false,
            websocket: false,
            ws_send_type: None,
            ws_receive_type: None,
        });

        a.extend(b);
        assert_eq!(a.len(), 2);
        assert_eq!(a.routes()[1].name, "bar");
    }
}
