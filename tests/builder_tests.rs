#![cfg(feature = "axum")]

use axfetchum::{ApiRouter, HttpMethod};
use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState;

#[derive(Deserialize)]
struct CreateUserRequest {
    _name: String,
}

#[derive(Serialize)]
struct UserResponse {
    _id: String,
}

#[derive(Deserialize)]
struct ListQuery {
    _page: Option<u32>,
}

async fn list_users(State(_state): State<AppState>) -> Json<Vec<UserResponse>> {
    Json(vec![])
}

async fn get_user(State(_state): State<AppState>, Path(_id): Path<String>) -> Json<UserResponse> {
    Json(UserResponse { _id: "1".into() })
}

async fn create_user(
    State(_state): State<AppState>,
    Json(_body): Json<CreateUserRequest>,
) -> Json<UserResponse> {
    Json(UserResponse { _id: "1".into() })
}

async fn delete_user(State(_state): State<AppState>, Path(_id): Path<String>) {}

// ---------------------------------------------------------------------------
// Auto-naming: handler function name → camelCase
// ---------------------------------------------------------------------------

#[test]
fn auto_name_from_handler() {
    let (_router, routes) = ApiRouter::<AppState>::new()
        .get("/users", list_users)
        .response::<Vec<UserResponse>>()
        .done()
        .build();

    // list_users → listUsers
    assert_eq!(routes.routes()[0].name, "listUsers");
}

#[test]
fn auto_name_single_word() {
    async fn register(State(_s): State<AppState>, Json(_b): Json<CreateUserRequest>) {}

    let (_router, routes) = ApiRouter::<AppState>::new()
        .post("/register", register)
        .body::<CreateUserRequest>()
        .done()
        .build();

    assert_eq!(routes.routes()[0].name, "register");
}

#[test]
fn as_overrides_auto_name() {
    let (_router, routes) = ApiRouter::<AppState>::new()
        .get("/users/{id}", get_user)
        .response::<UserResponse>()
        .as_("getById")
        .build();

    assert_eq!(routes.routes()[0].name, "getById");
}

// ---------------------------------------------------------------------------
// post_json / put_json / patch_json shorthand
// ---------------------------------------------------------------------------

#[test]
fn json_shorthand() {
    let (_router, routes) = ApiRouter::<AppState>::new()
        .post("/users", create_user)
        .json::<CreateUserRequest, UserResponse>()
        .auth()
        .done()
        .build();

    let r = &routes.routes()[0];
    assert_eq!(r.name, "createUser");
    assert_eq!(r.method, HttpMethod::Post);
    assert!(r.auth);
    assert!(r.body_type.as_ref().unwrap().contains("CreateUserRequest"));
    assert!(r.response_type.as_ref().unwrap().contains("UserResponse"));
}

#[test]
fn json_shorthand_put() {
    async fn update_user(
        State(_s): State<AppState>,
        Path(_id): Path<String>,
        Json(_b): Json<CreateUserRequest>,
    ) -> Json<UserResponse> {
        Json(UserResponse { _id: "1".into() })
    }

    let (_router, routes) = ApiRouter::<AppState>::new()
        .put("/users/{id}", update_user)
        .json::<CreateUserRequest, UserResponse>()
        .auth()
        .done()
        .build();

    let r = &routes.routes()[0];
    assert_eq!(r.name, "updateUser"); // auto snake→camel
    assert_eq!(r.method, HttpMethod::Put);
    assert!(r.body_type.as_ref().unwrap().contains("CreateUserRequest"));
    assert!(r.response_type.as_ref().unwrap().contains("UserResponse"));
    assert_eq!(r.path_params[0].name, "id");
}

// ---------------------------------------------------------------------------
// Full builder test (combines all features)
// ---------------------------------------------------------------------------

#[test]
fn builder_full_api() {
    let (_router, routes) = ApiRouter::<AppState>::new()
        .group("users")
        .get("/users", list_users)
        .response::<Vec<UserResponse>>()
        .auth()
        .done()
        .get("/users/{id}", get_user)
        .response::<UserResponse>()
        .auth()
        .as_("getById")
        .post("/users", create_user)
        .json::<CreateUserRequest, UserResponse>()
        .auth()
        .done()
        .delete("/users/{id}", delete_user)
        .auth()
        .done()
        .build();

    assert_eq!(routes.len(), 4);
    assert_eq!(routes.routes()[0].name, "listUsers");
    assert_eq!(routes.routes()[1].name, "getById"); // overridden
    assert_eq!(routes.routes()[2].name, "createUser");
    assert_eq!(routes.routes()[3].name, "deleteUser");

    // All grouped
    for r in routes.routes() {
        assert_eq!(r.group.as_deref(), Some("users"));
    }
}

// ---------------------------------------------------------------------------
// Group switching
// ---------------------------------------------------------------------------

#[test]
fn builder_group_switching() {
    let (_router, routes) = ApiRouter::<AppState>::new()
        .group("users")
        .get("/users", list_users)
        .response::<Vec<UserResponse>>()
        .done()
        .no_group()
        .get("/health", list_users)
        .as_("health")
        .group("admin")
        .delete("/users/{id}", delete_user)
        .auth()
        .done()
        .build();

    assert_eq!(routes.routes()[0].group.as_deref(), Some("users"));
    assert_eq!(routes.routes()[1].group, None);
    assert_eq!(routes.routes()[2].group.as_deref(), Some("admin"));
}

// ---------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------

#[test]
fn builder_merge() {
    let users = ApiRouter::<AppState>::new()
        .group("users")
        .get("/users", list_users)
        .response::<Vec<UserResponse>>()
        .done();

    let admin = ApiRouter::<AppState>::new()
        .group("admin")
        .delete("/users/{id}", delete_user)
        .auth()
        .done();

    let (_router, routes) = ApiRouter::<AppState>::new()
        .merge(users)
        .merge(admin)
        .build();

    assert_eq!(routes.len(), 2);
    assert_eq!(routes.routes()[0].group.as_deref(), Some("users"));
    assert_eq!(routes.routes()[1].group.as_deref(), Some("admin"));
}

// ---------------------------------------------------------------------------
// Query and redirect
// ---------------------------------------------------------------------------

#[test]
fn builder_query_type() {
    let (_router, routes) = ApiRouter::<AppState>::new()
        .get("/users", list_users)
        .query::<ListQuery>()
        .response::<Vec<UserResponse>>()
        .done()
        .build();

    let r = &routes.routes()[0];
    assert!(r.query_type.as_ref().unwrap().contains("ListQuery"));
}

#[test]
fn builder_redirect() {
    async fn authorize(State(_s): State<AppState>) {}

    let (_router, routes) = ApiRouter::<AppState>::new()
        .get("/oauth/{provider}/authorize", authorize)
        .redirect()
        .done()
        .build();

    let r = &routes.routes()[0];
    assert!(r.redirect);
    assert!(!r.auth);
    assert_eq!(r.path_params[0].name, "provider");
    assert_eq!(r.name, "authorize");
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

#[test]
fn builder_websocket_typed() {
    #[derive(Deserialize)]
    struct WsParams {
        _token: String,
    }

    #[derive(serde::Serialize)]
    struct ClientEvent {
        _msg: String,
    }

    #[derive(serde::Serialize)]
    struct ServerEvent {
        _reply: String,
    }

    async fn ws_upgrade(State(_s): State<AppState>) {}

    let (_router, routes) = ApiRouter::<AppState>::new()
        .ws("/ws", ws_upgrade)
        .query::<WsParams>()
        .events::<ClientEvent, ServerEvent>()
        .done()
        .build();

    let r = &routes.routes()[0];
    assert!(r.websocket);
    assert!(!r.redirect);
    assert!(!r.auth);
    assert!(r.query_type.as_ref().unwrap().contains("WsParams"));
    assert!(r.ws_send_type.as_ref().unwrap().contains("ClientEvent"));
    assert!(r.ws_receive_type.as_ref().unwrap().contains("ServerEvent"));
    assert_eq!(r.name, "wsUpgrade");
}

#[test]
fn builder_websocket_with_path_params() {
    #[derive(serde::Serialize)]
    struct ClientEvent {
        _msg: String,
    }

    #[derive(serde::Serialize)]
    struct ServerEvent {
        _reply: String,
    }

    async fn session_ws(State(_s): State<AppState>, Path(_id): Path<String>) {}

    let (_router, routes) = ApiRouter::<AppState>::new()
        .ws("/ws/{sessionId}", session_ws)
        .events::<ClientEvent, ServerEvent>()
        .done()
        .build();

    let r = &routes.routes()[0];
    assert!(r.websocket);
    assert_eq!(r.path_params[0].name, "sessionId");
    assert!(r.ws_send_type.as_ref().unwrap().contains("ClientEvent"));
    assert!(r.ws_receive_type.as_ref().unwrap().contains("ServerEvent"));
    assert_eq!(r.name, "sessionWs");
}

#[test]
fn builder_websocket_with_auth() {
    #[derive(serde::Serialize)]
    struct ClientEvent {
        _msg: String,
    }

    #[derive(serde::Serialize)]
    struct ServerEvent {
        _reply: String,
    }

    async fn ws_upgrade(State(_s): State<AppState>) {}

    let (_router, routes) = ApiRouter::<AppState>::new()
        .ws("/ws", ws_upgrade)
        .events::<ClientEvent, ServerEvent>()
        .auth()
        .done()
        .build();

    let r = &routes.routes()[0];
    assert!(r.websocket);
    assert!(r.auth);
}

#[test]
fn builder_websocket_custom_name() {
    #[derive(serde::Serialize)]
    struct ClientEvent {
        _msg: String,
    }

    #[derive(serde::Serialize)]
    struct ServerEvent {
        _reply: String,
    }

    async fn ws_handler(State(_s): State<AppState>) {}

    let (_router, routes) = ApiRouter::<AppState>::new()
        .ws("/ws", ws_handler)
        .events::<ClientEvent, ServerEvent>()
        .as_("connect")
        .build();

    let r = &routes.routes()[0];
    assert_eq!(r.name, "connect");
}

// ---------------------------------------------------------------------------
// End-to-end: generates valid TypeScript
// ---------------------------------------------------------------------------

#[test]
fn builder_generates_valid_ts() {
    let (_router, routes) = ApiRouter::<AppState>::new()
        .group("users")
        .get("/users", list_users)
        .response::<Vec<UserResponse>>()
        .auth()
        .done()
        .post("/users", create_user)
        .json::<CreateUserRequest, UserResponse>()
        .auth()
        .done()
        .build();

    let config = axfetchum::GeneratorConfig {
        factory_name: "createApiClient".into(),
        ..Default::default()
    };

    let output = axfetchum::generate(&routes, &config);
    assert!(output.contains("listUsers"));
    assert!(output.contains("createUser"));
    assert!(output.contains("UserResponse[]")); // Vec<UserResponse> → UserResponse[]
    assert!(output.contains("users:")); // group
}
