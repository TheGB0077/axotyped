use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::{Component, Path};
use std::process::Command;

use crate::types::{HttpMethod, RouteCollection, RouteDefinition};

/// Shorthand for `writeln!(...).unwrap()` — writing to `String` is infallible.
macro_rules! w {
    ($dst:expr) => { writeln!($dst).unwrap() };
    ($dst:expr, $($arg:tt)*) => { writeln!($dst, $($arg)*).unwrap() };
}

/// Configuration for the TypeScript client generator.
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Directory where ts-rs generates type bindings (e.g., `"./bindings"`).
    pub bindings_dir: String,
    /// Output path for the generated client file.
    pub output_path: String,
    /// Name of the factory function (e.g., `"createYAuthClient"`).
    pub factory_name: String,
    /// Whether to generate grouped nested objects.
    pub enable_groups: bool,
    /// Name of the error class (e.g., `"ApiError"` or `"YAuthError"`).
    pub error_class_name: String,
    /// Name of the options interface (e.g., `"ClientOptions"` or `"YAuthClientOptions"`).
    pub options_interface_name: String,
    /// Whether to include credentials by default.
    pub default_credentials: String,
    /// Import path prefix for types (relative from generated file to bindings dir).
    /// If empty, computed from bindings_dir relative to output_path.
    pub type_import_prefix: String,
    /// Optional shell command to format the generated file (e.g., `"biome format --write"`).
    /// The output file path is appended as the last argument.
    pub format_command: Option<String>,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            bindings_dir: "./bindings".into(),
            output_path: "./generated.ts".into(),
            factory_name: "createApiClient".into(),
            enable_groups: true,
            error_class_name: "ApiError".into(),
            options_interface_name: "ClientOptions".into(),
            default_credentials: "include".into(),
            type_import_prefix: String::new(),
            format_command: None,
        }
    }
}

/// Error returned by the check function when generated output doesn't match committed file.
#[derive(Debug)]
pub enum CheckError {
    /// Generated output differs from the committed file.
    OutOfSync { path: String },
    /// Could not read the committed file.
    ReadError { path: String, error: std::io::Error },
    /// Generation itself failed.
    GenerateError(String),
    /// The format command failed to execute.
    FormatError {
        command: String,
        error: std::io::Error,
    },
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfSync { path } => {
                write!(
                    f,
                    "Generated TypeScript client is out of sync with '{path}'. \
                     Run the generate command to update it."
                )
            }
            Self::ReadError { path, error } => {
                write!(f, "Failed to read '{path}': {error}")
            }
            Self::GenerateError(msg) => write!(f, "Generation error: {msg}"),
            Self::FormatError { command, error } => {
                write!(f, "Format command '{command}' failed: {error}")
            }
        }
    }
}

impl std::error::Error for CheckError {}

// ---------------------------------------------------------------------------
// Type helpers
// ---------------------------------------------------------------------------

const PRIMITIVES: &[&str] = &[
    "String", "&str", "Uuid", "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32",
    "f64", "usize", "isize",
];

/// Recursively strip `Vec<>`/`Option<>` wrappers, returning the innermost type name.
fn unwrap_inner(rust_type: &str) -> &str {
    let t = rust_type.trim();
    if let Some(inner) = t
        .strip_prefix("Vec<")
        .or_else(|| t.strip_prefix("Option<"))
        .and_then(|s| s.strip_suffix('>'))
    {
        return unwrap_inner(inner);
    }
    t
}

/// Convert a Rust type name (from `stringify!`) to a TypeScript type string.
fn rust_type_to_ts(rust_type: &str) -> String {
    let t = rust_type.trim();
    if let Some(inner) = t.strip_prefix("Vec<").and_then(|s| s.strip_suffix('>')) {
        return format!("{}[]", rust_type_to_ts(inner));
    }
    if let Some(inner) = t.strip_prefix("Option<").and_then(|s| s.strip_suffix('>')) {
        return format!("{} | null", rust_type_to_ts(inner));
    }
    match t {
        "String" | "&str" | "Uuid" => "string".into(),
        "bool" => "boolean".into(),
        _ if PRIMITIVES.contains(&t) => "number".into(),
        _ => t.to_string(),
    }
}

/// Check if a type is a primitive (doesn't need an import).
#[cfg(test)]
fn is_primitive_type(rust_type: &str) -> bool {
    PRIMITIVES.contains(&unwrap_inner(rust_type))
}

/// Get the base custom type name for imports (unwrapping Vec/Option, skipping primitives).
fn base_type_name(rust_type: &str) -> Option<&str> {
    let base = unwrap_inner(rust_type);
    if PRIMITIVES.contains(&base) {
        None
    } else {
        Some(base)
    }
}

// ---------------------------------------------------------------------------
// Path and parameter helpers
// ---------------------------------------------------------------------------

/// Compute the import path prefix for type imports.
fn compute_import_prefix(config: &GeneratorConfig) -> String {
    if !config.type_import_prefix.is_empty() {
        return config.type_import_prefix.clone();
    }

    let output_dir = Path::new(&config.output_path)
        .parent()
        .unwrap_or(Path::new("."));
    let bindings = Path::new(&config.bindings_dir);

    fn normal_parts(p: &Path) -> Vec<&str> {
        p.components()
            .filter_map(|c| match c {
                Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect()
    }

    let out_parts = normal_parts(output_dir);
    let bind_parts = normal_parts(bindings);

    let common = out_parts
        .iter()
        .zip(&bind_parts)
        .take_while(|(a, b)| a == b)
        .count();

    let ups = out_parts.len() - common;
    let mut prefix = if ups == 0 {
        "./".to_string()
    } else {
        "../".repeat(ups)
    };
    prefix.push_str(&bind_parts[common..].join("/"));
    prefix
}

/// Build the path template string for TypeScript.
///
/// `/admin/users/{id}` -> `` `/admin/users/${id}` ``
fn build_path_template(path: &str) -> String {
    if !path.contains('{') {
        return format!("\"{path}\"");
    }
    let mut template = String::new();
    for ch in path.chars() {
        if ch == '{' {
            template.push_str("${");
        } else {
            template.push(ch);
        }
    }
    format!("`{template}`")
}

/// Generate a function's parameter list for a route.
fn generate_params(route: &RouteDefinition) -> String {
    let mut params = Vec::new();
    for param in &route.path_params {
        params.push(format!("{}: string", param.name));
    }
    if let Some(ref body_type) = route.body_type {
        params.push(format!("body: {}", rust_type_to_ts(body_type)));
    }
    if let Some(ref query_type) = route.query_type {
        params.push(format!("query?: {}", rust_type_to_ts(query_type)));
    }
    params.join(", ")
}

/// Generate the request options object literal for a route.
fn generate_request_options(route: &RouteDefinition) -> String {
    let mut opts = Vec::new();
    if route.method != HttpMethod::Get {
        opts.push(format!("method: \"{}\"", route.method.as_str()));
    }
    if route.auth {
        opts.push("auth: true".into());
    }
    if route.body_type.is_some() {
        opts.push("body".into());
    }
    if route.query_type.is_some() {
        opts.push("query".into());
    }
    if opts.is_empty() {
        String::new()
    } else {
        format!(", {{ {} }}", opts.join(", "))
    }
}

// ---------------------------------------------------------------------------
// Template constants for static TypeScript blocks
// ---------------------------------------------------------------------------

const ERROR_CLASS: &str = r#"export class __ERROR__ extends Error {
  constructor(message: string, public status: number, public body?: unknown) {
    super(message);
    this.name = "__ERROR__";
  }
}
"#;

const OPTIONS_INTERFACE: &str = r#"export interface __OPTS__ {
  baseUrl: string;
  getToken?: () => Promise<string | null>;
  credentials?: RequestCredentials;
  fetch?: typeof fetch;
  onError?: (error: __ERROR__) => void;
}
"#;

const REQUEST_OPTIONS_TYPE: &str = "\
type RequestOptions = {
  method?: string;
  body?: unknown;
  query?: Record<string, unknown>;
  auth?: boolean;
};
";

const REQUEST_HELPER: &str = r#"function createRequest(options: __OPTS__) {
  const { baseUrl, credentials = "__CREDS__" } = options;
  async function request<T>(
    path: string,
    opts: RequestOptions = {},
  ): Promise<T> {
    const { method = "GET", body, query, auth } = opts;
    // Resolve fetch at call time (not at client creation) so OTel
    // instrumentation patches are picked up even when the client
    // module is imported before telemetry initializes.
    const fetchFn = options.fetch ?? globalThis.fetch;

    let url = `${baseUrl}${path}`;
    if (query) {
      const params = new URLSearchParams();
      for (const [key, value] of Object.entries(query)) {
        if (value !== undefined && value !== null) {
          params.set(key, String(value));
        }
      }
      const qs = params.toString();
      if (qs) url += `?${qs}`;
    }

    const headers: Record<string, string> = { "Content-Type": "application/json" };

    if (auth && options.getToken) {
      const token = await options.getToken();
      if (token) headers.Authorization = `Bearer ${token}`;
    }

    const response = await fetchFn(url, {
      method,
      credentials,
      headers,
      body: body ? JSON.stringify(body) : undefined,
    });

    if (!response.ok) {
      const text = await response.text();
      let message: string;
      let errorBody: unknown;
      try {
        const json = JSON.parse(text);
        message = json.error ?? json.message ?? text;
        errorBody = json;
      } catch {
        message = text;
      }
      const error = new __ERROR__(message, response.status, errorBody);
      if (options.onError) options.onError(error);
      throw error;
    }

    const text = await response.text();
    return (text ? JSON.parse(text) : undefined) as T;
  }

  return request;
}
"#;

const TYPED_WS_INTERFACE: &str = r#"export interface TypedWebSocket<TSend, TReceive> {
  send(event: TSend): void;
  onOpen(handler: () => void): void;
  onMessage(handler: (event: TReceive) => void): void;
  onError(handler: (event: Event) => void): void;
  onClose(handler: (event: CloseEvent) => void): void;
  close(code?: number, reason?: string): void;
  readonly readyState: number;
}
"#;

const TYPED_WS_HELPER: &str = r#"function createTypedWebSocket<TSend, TReceive>(ws: WebSocket): TypedWebSocket<TSend, TReceive> {
  return {
    send(event: TSend) { ws.send(JSON.stringify(event)); },
    onOpen(handler: () => void) { ws.addEventListener("open", handler); },
    onMessage(handler: (event: TReceive) => void) {
      ws.addEventListener("message", (raw: MessageEvent) => {
        handler(JSON.parse(raw.data as string) as TReceive);
      });
    },
    onError(handler: (event: Event) => void) { ws.addEventListener("error", handler); },
    onClose(handler: (event: CloseEvent) => void) { ws.addEventListener("close", handler); },
    close(code?: number, reason?: string) { ws.close(code, reason); },
    get readyState() { return ws.readyState; },
  };
}
"#;

// ---------------------------------------------------------------------------
// Code generation
// ---------------------------------------------------------------------------

/// Generate the full TypeScript client source code.
pub fn generate(routes: &RouteCollection, config: &GeneratorConfig) -> String {
    let mut out = String::new();

    // Header
    w!(out, "// Auto-generated by axfetchum. Do not edit.");
    w!(out);

    // Collect custom types for import (includes ws_send/ws_receive types)
    let import_prefix = compute_import_prefix(config);
    let custom_types: BTreeSet<&str> = routes
        .iter()
        .flat_map(|r| {
            [
                r.body_type.as_deref(),
                r.response_type.as_deref(),
                r.query_type.as_deref(),
                r.ws_send_type.as_deref(),
                r.ws_receive_type.as_deref(),
            ]
        })
        .flatten()
        .filter_map(base_type_name)
        .collect();

    for type_name in &custom_types {
        w!(
            out,
            "import type {{ {type_name} }} from \"{import_prefix}/{type_name}\";"
        );
    }
    if !custom_types.is_empty() {
        w!(out);
    }

    // Static blocks via template substitution
    let error_name = &config.error_class_name;
    let opts_name = &config.options_interface_name;
    let default_creds = &config.default_credentials;

    let substitute = |template: &str| -> String {
        template
            .replace("__ERROR__", error_name)
            .replace("__OPTS__", opts_name)
            .replace("__CREDS__", default_creds)
    };

    out.push_str(&substitute(ERROR_CLASS));
    w!(out);
    out.push_str(&substitute(OPTIONS_INTERFACE));
    w!(out);
    out.push_str(REQUEST_OPTIONS_TYPE);
    w!(out);
    out.push_str(&substitute(REQUEST_HELPER));
    w!(out);

    // Typed WebSocket helpers — only if any WS route exists
    let has_ws = routes.iter().any(|r| r.websocket);
    if has_ws {
        out.push_str(TYPED_WS_INTERFACE);
        w!(out);
        out.push_str(TYPED_WS_HELPER);
        w!(out);
    }

    // Factory function
    let factory = &config.factory_name;
    w!(out, "export function {factory}(options: {opts_name}) {{");
    w!(out, "  const request = createRequest(options);");
    w!(out);
    w!(out, "  return {{");

    if config.enable_groups {
        generate_grouped_routes(&mut out, routes);
    } else {
        generate_flat_routes(&mut out, routes);
    }

    w!(out, "  }};");
    w!(out, "}}");
    w!(out);

    // Type export
    let type_name = derive_type_name(factory);
    w!(
        out,
        "export type {type_name} = ReturnType<typeof {factory}>;"
    );

    out
}

/// Generate routes organized into groups (nested objects).
fn generate_grouped_routes(out: &mut String, routes: &RouteCollection) {
    let mut ungrouped: Vec<&RouteDefinition> = Vec::new();
    let mut groups: BTreeMap<String, Vec<&RouteDefinition>> = BTreeMap::new();

    for route in routes {
        match &route.group {
            Some(group) => groups.entry(group.clone()).or_default().push(route),
            None => ungrouped.push(route),
        }
    }

    for route in &ungrouped {
        write!(out, "    ").unwrap();
        generate_route_method(out, route, 4);
        w!(out, ",");
    }

    for (group_name, group_routes) in &groups {
        if !ungrouped.is_empty() || groups.keys().next() != Some(group_name) {
            w!(out);
        }
        w!(out, "    {group_name}: {{");
        for route in group_routes {
            write!(out, "      ").unwrap();
            generate_route_method(out, route, 6);
            w!(out, ",");
        }
        w!(out, "    }},");
    }
}

/// Generate routes in a flat structure (no grouping).
fn generate_flat_routes(out: &mut String, routes: &RouteCollection) {
    for route in routes {
        write!(out, "    ").unwrap();
        generate_route_method(out, route, 4);
        w!(out, ",");
    }
}

/// Generate a single route method.
fn generate_route_method(out: &mut String, route: &RouteDefinition, indent: usize) {
    if route.websocket {
        generate_ws_method(out, route, indent);
    } else if route.redirect {
        generate_redirect_method(out, route, indent);
    } else {
        let name = &route.name;
        let params = generate_params(route);
        let path_template = build_path_template(&route.path);
        let return_type = route
            .response_type
            .as_ref()
            .map(|t| rust_type_to_ts(t))
            .unwrap_or_else(|| "void".into());
        let opts = generate_request_options(route);

        if params.is_empty() {
            write!(
                out,
                "{name}: () => request<{return_type}>({path_template}{opts})"
            )
            .unwrap();
        } else {
            let pad = " ".repeat(indent + 2);
            write!(
                out,
                "{name}: ({params}) =>\n{pad}request<{return_type}>({path_template}{opts})"
            )
            .unwrap();
        }
    }
}

/// Generate a redirect route (URL-builder, not fetch).
fn generate_redirect_method(out: &mut String, route: &RouteDefinition, indent: usize) {
    let name = &route.name;
    let path_template = build_path_template(&route.path);
    let path_inner = &path_template[1..path_template.len() - 1];
    let pad = " ".repeat(indent);
    let pad2 = " ".repeat(indent + 2);
    let pad4 = " ".repeat(indent + 4);

    if let Some(ref query_type) = route.query_type {
        let query_ts = rust_type_to_ts(query_type);
        let mut fn_params = Vec::new();
        for param in &route.path_params {
            fn_params.push(format!("{}: string", param.name));
        }
        fn_params.push(format!("query?: {query_ts}"));
        let all_params = fn_params.join(", ");

        w!(out, "{name}: ({all_params}) => {{");
        w!(out, "{pad2}let url = `${{options.baseUrl}}{path_inner}`;");
        w!(out, "{pad2}if (query) {{");
        w!(out, "{pad4}const params = new URLSearchParams();");
        w!(
            out,
            "{pad4}for (const [key, value] of Object.entries(query)) {{"
        );
        w!(
            out,
            "{pad4}  if (value !== undefined && value !== null) params.set(key, String(value));"
        );
        w!(out, "{pad4}}}");
        w!(out, "{pad4}const qs = params.toString();");
        w!(out, "{pad4}if (qs) url += `?${{qs}}`;");
        w!(out, "{pad2}}}");
        write!(out, "{pad2}return url;\n{pad}}}").unwrap();
    } else {
        let params = generate_params(route);
        if params.is_empty() {
            write!(out, "{name}: () => `${{options.baseUrl}}{path_inner}`").unwrap();
        } else {
            write!(
                out,
                "{name}: ({params}) => `${{options.baseUrl}}{path_inner}`"
            )
            .unwrap();
        }
    }
}

/// Derive a type name from a factory function name.
///
/// `createYAuthClient` -> `YAuthClient`
fn derive_type_name(factory_name: &str) -> String {
    factory_name
        .strip_prefix("create")
        .unwrap_or(factory_name)
        .to_string()
}

/// Generate a WebSocket route method.
///
/// When send/receive types are defined, produces a TypeScript factory that
/// returns a `TypedWebSocket<S, R>` with typed `send()` and `onMessage()`.
/// Otherwise produces a bare `new WebSocket(url)`.
fn generate_ws_method(out: &mut String, route: &RouteDefinition, indent: usize) {
    let name = &route.name;
    let path_template = build_path_template(&route.path);
    let path_inner = &path_template[1..path_template.len() - 1];
    let pad = " ".repeat(indent);
    let pad2 = " ".repeat(indent + 2);
    let pad4 = " ".repeat(indent + 4);

    // Build function parameters: path params + optional query
    let mut fn_params = Vec::new();
    for param in &route.path_params {
        fn_params.push(format!("{}: string", param.name));
    }
    if let Some(ref query_type) = route.query_type {
        fn_params.push(format!("query?: {}", rust_type_to_ts(query_type)));
    }
    let all_params = fn_params.join(", ");

    // Determine return type
    let has_types = route.ws_send_type.is_some() && route.ws_receive_type.is_some();
    let return_type = if has_types {
        let send_ts = rust_type_to_ts(route.ws_send_type.as_ref().unwrap());
        let recv_ts = rust_type_to_ts(route.ws_receive_type.as_ref().unwrap());
        format!("TypedWebSocket<{}, {}>", send_ts, recv_ts)
    } else {
        "WebSocket".into()
    };

    w!(out, "{name}: ({all_params}): {return_type} => {{");

    // Convert http(s) to ws(s)
    w!(out, "{pad2}const baseUrl = options.baseUrl.replace(/^http/, (m) => m === \"https\" ? \"wss\" : \"ws\");");

    // Build path with params
    w!(out, "{pad2}let url = `${{baseUrl}}{path_inner}`;");

    // Append query params if present
    if route.query_type.is_some() {
        w!(out, "{pad2}if (query) {{");
        w!(out, "{pad4}const params = new URLSearchParams();");
        w!(
            out,
            "{pad4}for (const [key, value] of Object.entries(query)) {{"
        );
        w!(
            out,
            "{pad4}  if (value !== undefined && value !== null) params.set(key, String(value));"
        );
        w!(out, "{pad4}}}");
        w!(out, "{pad4}const qs = params.toString();");
        w!(out, "{pad4}if (qs) url += `?${{qs}}`;");
        w!(out, "{pad2}}}");
    }

    if has_types {
        w!(out, "{pad2}const ws = new WebSocket(url);");
        let send_ts = rust_type_to_ts(route.ws_send_type.as_ref().unwrap());
        let recv_ts = rust_type_to_ts(route.ws_receive_type.as_ref().unwrap());
        w!(out, "{pad2}return createTypedWebSocket<{send_ts}, {recv_ts}>(ws);");
    } else {
        w!(out, "{pad2}return new WebSocket(url);");
    }

    write!(out, "{pad}}}").unwrap();
}

// ---------------------------------------------------------------------------
// File I/O and check
// ---------------------------------------------------------------------------

/// Run a format command on the given file path.
fn run_format_command(format_command: &str, file_path: &str) -> Result<(), std::io::Error> {
    let parts: Vec<&str> = format_command.split_whitespace().collect();
    if parts.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "format_command is empty",
        ));
    }

    let output = Command::new(parts[0])
        .args(&parts[1..])
        .arg(file_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(format!(
            "format command '{}' exited with {}: {}",
            format_command,
            output.status,
            stderr.trim()
        )));
    }

    Ok(())
}

/// Generate the client and write it to a file.
///
/// If `config.format_command` is set, the format command is run on the output file after writing.
pub fn generate_to_file(
    routes: &RouteCollection,
    config: &GeneratorConfig,
) -> Result<(), std::io::Error> {
    let content = generate(routes, config);

    if let Some(parent) = Path::new(&config.output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&config.output_path, &content)?;

    if let Some(ref cmd) = config.format_command {
        run_format_command(cmd, &config.output_path)?;
    }

    Ok(())
}

/// RAII guard that removes a temp file on drop.
struct TempFile(std::path::PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Check if the generated output matches the committed file.
///
/// When `config.format_command` is set, the generated output is written to a temporary file
/// and formatted before comparing, so the check accounts for formatter changes.
///
/// Returns `Ok(())` if in sync, `Err(CheckError)` if not.
pub fn check(routes: &RouteCollection, config: &GeneratorConfig) -> Result<(), CheckError> {
    let generated = generate(routes, config);

    let expected = if let Some(ref cmd) = config.format_command {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let temp = TempFile(
            std::env::temp_dir().join(format!("axfetchum_check_{}_{nanos}.ts", std::process::id())),
        );
        let temp_str = temp.0.to_string_lossy().to_string();

        std::fs::write(&temp.0, &generated).map_err(|e| CheckError::ReadError {
            path: temp_str.clone(),
            error: e,
        })?;

        run_format_command(cmd, &temp_str).map_err(|e| CheckError::FormatError {
            command: cmd.clone(),
            error: e,
        })?;

        std::fs::read_to_string(&temp.0).map_err(|e| CheckError::ReadError {
            path: temp_str,
            error: e,
        })?
        // temp file auto-removed on drop
    } else {
        generated
    };

    let existing =
        std::fs::read_to_string(&config.output_path).map_err(|e| CheckError::ReadError {
            path: config.output_path.clone(),
            error: e,
        })?;

    if expected != existing {
        Err(CheckError::OutOfSync {
            path: config.output_path.clone(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_type_to_ts_primitives() {
        assert_eq!(rust_type_to_ts("String"), "string");
        assert_eq!(rust_type_to_ts("bool"), "boolean");
        assert_eq!(rust_type_to_ts("u32"), "number");
        assert_eq!(rust_type_to_ts("i64"), "number");
        assert_eq!(rust_type_to_ts("Uuid"), "string");
        assert_eq!(rust_type_to_ts("f64"), "number");
    }

    #[test]
    fn test_rust_type_to_ts_vec() {
        assert_eq!(rust_type_to_ts("Vec<String>"), "string[]");
        assert_eq!(rust_type_to_ts("Vec<UserResponse>"), "UserResponse[]");
    }

    #[test]
    fn test_rust_type_to_ts_option() {
        assert_eq!(rust_type_to_ts("Option<String>"), "string | null");
        assert_eq!(
            rust_type_to_ts("Option<UserResponse>"),
            "UserResponse | null"
        );
    }

    #[test]
    fn test_rust_type_to_ts_custom() {
        assert_eq!(rust_type_to_ts("RegisterRequest"), "RegisterRequest");
        assert_eq!(rust_type_to_ts("LoginResponse"), "LoginResponse");
    }

    #[test]
    fn test_build_path_template_no_params() {
        assert_eq!(build_path_template("/register"), "\"/register\"");
        assert_eq!(build_path_template("/admin/users"), "\"/admin/users\"");
    }

    #[test]
    fn test_build_path_template_with_params() {
        assert_eq!(
            build_path_template("/admin/users/{id}"),
            "`/admin/users/${id}`"
        );
        assert_eq!(
            build_path_template("/orgs/{org}/users/{id}"),
            "`/orgs/${org}/users/${id}`"
        );
    }

    #[test]
    fn test_derive_type_name() {
        assert_eq!(derive_type_name("createYAuthClient"), "YAuthClient");
        assert_eq!(derive_type_name("createApiClient"), "ApiClient");
        assert_eq!(derive_type_name("myClient"), "myClient");
    }

    #[test]
    fn test_is_primitive_type() {
        assert!(is_primitive_type("String"));
        assert!(is_primitive_type("bool"));
        assert!(is_primitive_type("u32"));
        assert!(is_primitive_type("Uuid"));
        assert!(!is_primitive_type("RegisterRequest"));
        assert!(is_primitive_type("Vec<String>"));
        assert!(!is_primitive_type("Vec<UserResponse>"));
    }

    #[test]
    fn test_base_type_name() {
        assert_eq!(base_type_name("String"), None);
        assert_eq!(base_type_name("RegisterRequest"), Some("RegisterRequest"));
        assert_eq!(base_type_name("Vec<UserResponse>"), Some("UserResponse"));
        assert_eq!(base_type_name("Vec<String>"), None);
        assert_eq!(base_type_name("Option<UserResponse>"), Some("UserResponse"));
    }

    #[test]
    fn test_compute_import_prefix_explicit() {
        let config = GeneratorConfig {
            type_import_prefix: "../types".into(),
            ..Default::default()
        };
        assert_eq!(compute_import_prefix(&config), "../types");
    }
}
