# axotyped

Auto-generate typed TypeScript API clients from Axum route metadata.

## Quick start

Annotate your handlers with `#[endpoint]`, register them with `register!()`, and get both an Axum router and a typed TypeScript client:

```rust
use axotyped::{ApiRouter, endpoint, register};

// Derive axotyped::TS (re-exported ts-rs) for any types crossing the wire
#[derive(serde::Serialize, axotyped::TS)]
pub struct ProjectResponse {
    pub id: String,
    pub title: String,
}

// #[endpoint] extracts Json<T>, Query<T>, and response types from the signature
#[endpoint]
pub async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProjectResponse>>, StatusCode> {
    // ...
}

#[endpoint]
pub async fn create_project(
    State(state): State<AppState>,
    Json(body): Json<CreateProjectRequest>,
) -> Result<Json<ProjectResponse>, StatusCode> {
    // ...
}

// register!() wires the handler into both the Axum router and type metadata
fn routes() -> (Router<AppState>, RouteCollection) {
    ApiRouter::<AppState>::new()
        .group_with("projects", |g| {
            g.auth_all()
             .get("/projects", register!(list_projects))
                 .done()
             .post("/projects", register!(create_project))
                 .done()
        })
        .build()
}
```

Generates a TypeScript client:

```typescript
const api = createApiClient({
  baseUrl: "http://localhost:3000",
  getToken: async () => localStorage.getItem("token"),
});

const projects = await api.projects.listProjects();
const newProject = await api.projects.createProject({ title: "My Project" });
```

Handler names auto-convert to camelCase: `list_projects` → `listProjects`, `create_project` → `createProject`. Override with `.as_("customName")` when needed.

## Installation

```toml
[dependencies]
axotyped = { version = "0.2", features = ["ts-rs"] }
```

- **`ts-rs`** — enables TypeScript type export for your structs/enums (recommended)

Without `ts-rs`, type collection is a no-op — routes are still registered but no `.ts` type files are generated.

`axotyped` re-exports `ts-rs` as `axotyped::TS`, so you don't need a separate `ts-rs` dependency.

## How it works

**Two crates work together:**

| Crate | Generates | Purpose |
|---|---|---|
| **`axotyped`** | `generated.ts` — typed fetch wrappers | Route definitions, client factory, error handling |
| **[`ts-rs`](https://crates.io/crates/ts-rs)** | Individual `.ts` type files | TypeScript interfaces for your Rust structs |

`axotyped` generates `import type { ProjectResponse } from "./bindings/ProjectResponse"` — those files come from `ts-rs`.

## Defining routes

### Recommended: `#[endpoint]` + `register!()`

Annotate handlers with `#[endpoint]` and pass them to `register!()` inside the builder. Types are inferred from the function signature — no manual specification needed.

```rust
use axotyped::{ApiRouter, endpoint, register};

#[derive(serde::Deserialize, axotyped::TS)]
pub struct CreateProjectRequest {
    pub title: String,
}

#[endpoint]
pub async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateProjectRequest>,
) -> Result<Json<ProjectResponse>, StatusCode> {
    // ...
}

#[endpoint]
pub async fn list_projects(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ProjectResponse>>, StatusCode> {
    // ...
}

#[endpoint]
pub async fn delete_project(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, StatusCode> {
    // ...
}

ApiRouter::<Arc<AppState>>::new()
    .group_with("admin", |g| {
        g.auth_all()
         .post("/project", register!(create_project))
             .done()
         .get("/project", register!(list_projects))
             .done()
         .delete("/project/{key}", register!(delete_project))
             .done()
    })
    .build()
```

**How type inference works:**

| Signature pattern | Extracted type |
|---|---|
| `Json(body): Json<T>` in params | Body type: `T` |
| `Query(params): Query<T>` in params | Query type: `T` |
| `-> Result<Json<T>, E>` | Response type: `T` |
| `-> Result<StatusCode, StatusCode>` | No response (void) |
| `-> Json<T>` (non-Result) | Response type: `T` |

Inner types of `Vec<T>` and `Option<T>` are automatically collected for TypeScript export, so `Vec<ProjectTag>` correctly generates `ProjectTag.ts`.

`impl Trait` types (e.g. `Result<impl IntoResponse, E>`) are gracefully skipped — no metadata is generated, compilation is not affected.

### Grouping routes

**`.group(name)`** — sets the TypeScript namespace for subsequent routes (no URL prefix):

```rust
ApiRouter::<AppState>::new()
    .group("admin")
    .get("/reports", register!(list_reports))
        .auth()
        .done()
    // ...
```

**`.group_with(name, closure)`** — closure-based grouping with scoped URL prefix, default auth, and TypeScript namespace. The group's config does not leak to routes registered after the closure.

The prefix defaults to `"/{name}"` but can be overridden with `.set_prefix()` inside the closure.

```rust
ApiRouter::<Arc<AppState>>::new()
    .group_with("admin", |g| {
        g.auth_all()
         .post("/project", register!(create_project))
             .done()
         .get("/project", register!(list_projects))
             .done()
         .delete("/project/{key}", register!(delete_project))
             .done()
    })
    .get("/health", register!(health))
        .done()
    .build()
// admin routes: POST /admin/project, GET /admin/project, DELETE /admin/project/{key}
// health route: GET /health (no prefix, no auth, no group)
```

### Manual type specification

You can still specify types explicitly on the builder when needed:

```rust
ApiRouter::<AppState>::new()
    .get("/projects", list_projects)
        .response::<Vec<ProjectResponse>>()
        .auth()
        .done()
    .post("/projects", create_project)
        .json::<CreateProjectRequest, ProjectResponse>()
        .auth()
        .done()
    .build()
```

## Generating the client

Generation runs at build time — call it from `main.rs` or a `#[test]` after your routes are defined.

```rust
use axotyped::GeneratorConfig;

fn config() -> GeneratorConfig {
    GeneratorConfig {
        bindings_dir: "./bindings".into(),
        output_path: "./packages/client/src/generated.ts".into(),
        factory_name: "createApiClient".into(),
        error_class_name: "ApiError".into(),
        options_interface_name: "ApiClientOptions".into(),
        type_import_prefix: "./bindings".into(),
        format_command: Some("bun biome check --write --unsafe".into()),
        ..Default::default()
    }
}

// Export ts-rs type files, then generate the client:
fn generate_ts_client(routes: &axotyped::RouteCollection) {
    use axotyped::GeneratorConfig;

    let bindings_dir = std::path::Path::new("./bindings");
    routes.export_types(&bindings_dir).unwrap();
    axotyped::generate_to_file(routes, &config()).unwrap();
}

// CI check — fails if the committed file is stale:
fn check_ts_client(routes: &axotyped::RouteCollection) {
    axotyped::check(routes, &config())
        .expect("Generated TypeScript client is out of date!");
}
```

## GeneratorConfig

| Field | Default | Description |
|---|---|---|
| `bindings_dir` | `"./bindings"` | Where `ts-rs` writes type files |
| `output_path` | `"./generated.ts"` | Where to write the generated client |
| `factory_name` | `"createApiClient"` | Name of the factory function |
| `enable_groups` | `true` | Nest routes into group objects |
| `error_class_name` | `"ApiError"` | Name of the generated error class |
| `options_interface_name` | `"ClientOptions"` | Name of the options interface |
| `default_credentials` | `"include"` | Default `RequestCredentials` value |
| `type_import_prefix` | (computed) | Import path from generated file to bindings dir |
| `format_command` | `None` | Shell command to format after generation |

## Generated output

The generated client includes:
- Type imports from `ts-rs` bindings
- A typed error class (extends `Error` with `status` and `body`)
- A client options interface (`baseUrl`, `getToken`, `credentials`, `fetch`, `onError`)
- A factory function returning typed fetch methods with optional grouping
- A type alias: `export type ApiClient = ReturnType<typeof createApiClient>`

See [tests/snapshots/yauth_style.ts](tests/snapshots/yauth_style.ts) for a full example.

## License

MIT
