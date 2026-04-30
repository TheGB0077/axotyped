use axfetchum::{HttpMethod, api_routes};

#[test]
fn simple_get_route() {
    let routes = api_routes! {
        getSession: GET "/session" [auth]
            -> SessionResponse;
    };
    assert_eq!(routes.len(), 1);
    let r = &routes.routes()[0];
    assert_eq!(r.name, "getSession");
    assert_eq!(r.method, HttpMethod::Get);
    assert_eq!(r.path, "/session");
    assert!(r.auth);
    assert_eq!(r.response_type.as_deref(), Some("SessionResponse"));
    assert!(r.body_type.is_none());
    assert!(r.query_type.is_none());
    assert!(r.group.is_none());
    assert!(!r.redirect);
    assert!(!r.websocket);
}

#[test]
fn post_with_body_and_response() {
    let routes = api_routes! {
        register: POST "/register"
            body: RegisterRequest -> MessageResponse;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.name, "register");
    assert_eq!(r.method, HttpMethod::Post);
    assert!(!r.auth);
    assert_eq!(r.body_type.as_deref(), Some("RegisterRequest"));
    assert_eq!(r.response_type.as_deref(), Some("MessageResponse"));
}

#[test]
fn route_with_group() {
    let routes = api_routes! {
        @group emailPassword

        register: POST "/register"
            body: RegisterRequest -> MessageResponse;
        login: POST "/login"
            body: LoginRequest -> LoginResponse;
    };
    assert_eq!(routes.len(), 2);
    assert_eq!(routes.routes()[0].group.as_deref(), Some("emailPassword"));
    assert_eq!(routes.routes()[1].group.as_deref(), Some("emailPassword"));
}

#[test]
fn route_with_path_params() {
    let routes = api_routes! {
        getUser: GET "/admin/users/{id}" [auth]
            -> UserResponse;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.path_params.len(), 1);
    assert_eq!(r.path_params[0].name, "id");
}

#[test]
fn route_with_query_params() {
    let routes = api_routes! {
        listUsers: GET "/admin/users" [auth]
            query: ListUsersQuery -> ListUsersResponse;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.query_type.as_deref(), Some("ListUsersQuery"));
    assert_eq!(r.response_type.as_deref(), Some("ListUsersResponse"));
    assert!(r.auth);
}

#[test]
fn redirect_route() {
    let routes = api_routes! {
        authorize: GET "/oauth/{provider}/authorize" [redirect]
            query: AuthorizeQuery;
    };
    let r = &routes.routes()[0];
    assert!(r.redirect);
    assert!(!r.auth);
    assert_eq!(r.path_params.len(), 1);
    assert_eq!(r.path_params[0].name, "provider");
    assert_eq!(r.query_type.as_deref(), Some("AuthorizeQuery"));
    assert!(r.response_type.is_none());
}

#[test]
fn websocket_route() {
    let routes = api_routes! {
        wsUpgrade: GET "/ws" [ws]
            send: ClientEvent, receive: ServerEvent
            query: WsParams;
    };
    let r = &routes.routes()[0];
    assert!(r.websocket);
    assert!(!r.redirect);
    assert!(!r.auth);
    assert_eq!(r.query_type.as_deref(), Some("WsParams"));
    assert_eq!(r.ws_send_type.as_deref(), Some("ClientEvent"));
    assert_eq!(r.ws_receive_type.as_deref(), Some("ServerEvent"));
    assert!(r.response_type.is_none());
}

#[test]
fn websocket_with_auth() {
    let routes = api_routes! {
        wsConnect: GET "/ws/{sessionId}" [auth, ws]
            send: ClientEvent, receive: ServerEvent
            query: WsParams;
    };
    let r = &routes.routes()[0];
    assert!(r.websocket);
    assert!(r.auth);
    assert_eq!(r.path_params[0].name, "sessionId");
    assert_eq!(r.ws_send_type.as_deref(), Some("ClientEvent"));
    assert_eq!(r.ws_receive_type.as_deref(), Some("ServerEvent"));
}

#[test]
fn multiple_flags() {
    let routes = api_routes! {
        protectedRedirect: GET "/oauth/{provider}/link" [auth, redirect]
            -> LinkResponse;
    };
    let r = &routes.routes()[0];
    assert!(r.auth);
    assert!(r.redirect);
}

#[test]
fn nogroup_clears_context() {
    let routes = api_routes! {
        @group myGroup

        a: GET "/a" -> AResponse;

        @nogroup

        b: GET "/b" -> BResponse;
    };
    assert_eq!(routes.routes()[0].group.as_deref(), Some("myGroup"));
    assert_eq!(routes.routes()[1].group, None);
}

#[test]
fn multiple_groups() {
    let routes = api_routes! {
        @group emailPassword

        register: POST "/register"
            body: RegisterRequest -> MessageResponse;

        @group passkey

        loginBegin: POST "/passkey/login/begin"
            body: PasskeyLoginBeginRequest -> PasskeyLoginBeginResponse;
    };
    assert_eq!(routes.len(), 2);
    assert_eq!(routes.routes()[0].group.as_deref(), Some("emailPassword"));
    assert_eq!(routes.routes()[1].group.as_deref(), Some("passkey"));
}

#[test]
fn delete_method() {
    let routes = api_routes! {
        deletePasskey: DELETE "/passkeys/{id}" [auth]
            -> MessageResponse;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.method, HttpMethod::Delete);
}

#[test]
fn put_method() {
    let routes = api_routes! {
        updateUser: PUT "/admin/users/{id}" [auth]
            body: UpdateUserRequest -> UserResponse;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.method, HttpMethod::Put);
}

#[test]
fn patch_method() {
    let routes = api_routes! {
        updateProfile: PATCH "/me" [auth]
            body: UpdateProfileRequest -> ProfileResponse;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.method, HttpMethod::Patch);
}

#[test]
fn no_response_type() {
    let routes = api_routes! {
        deleteItem: DELETE "/items/{id}" [auth];
    };
    let r = &routes.routes()[0];
    assert!(r.response_type.is_none());
    assert!(r.body_type.is_none());
}

#[test]
fn body_only_no_response() {
    let routes = api_routes! {
        doSomething: POST "/action"
            body: ActionRequest;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.body_type.as_deref(), Some("ActionRequest"));
    assert!(r.response_type.is_none());
}

#[test]
fn vec_response_type() {
    let routes = api_routes! {
        listUsers: GET "/admin/users" [auth]
            -> Vec<UserResponse>;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.response_type.as_deref(), Some("Vec<UserResponse>"));
}

#[test]
fn option_response_type() {
    let routes = api_routes! {
        getUser: GET "/users/{id}" [auth]
            -> Option<UserResponse>;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.response_type.as_deref(), Some("Option<UserResponse>"));
}

#[test]
fn vec_body_type() {
    let routes = api_routes! {
        batchCreate: POST "/items"
            body: Vec<CreateItemRequest> -> Vec<ItemResponse>;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.body_type.as_deref(), Some("Vec<CreateItemRequest>"));
    assert_eq!(r.response_type.as_deref(), Some("Vec<ItemResponse>"));
}

#[test]
fn vec_query_type() {
    let routes = api_routes! {
        listRuns: GET "/api/runs" [auth]
            query: RunListQuery -> Vec<RunResponse>;
    };
    let r = &routes.routes()[0];
    assert_eq!(r.query_type.as_deref(), Some("RunListQuery"));
    assert_eq!(r.response_type.as_deref(), Some("Vec<RunResponse>"));
}

#[test]
fn collection_extend() {
    let mut core = api_routes! {
        getSession: GET "/session" [auth]
            -> SessionResponse;
    };

    let email_password = api_routes! {
        @group emailPassword
        register: POST "/register"
            body: RegisterRequest -> MessageResponse;
    };

    core.extend(email_password);
    assert_eq!(core.len(), 2);
    assert!(core.routes()[0].group.is_none());
    assert_eq!(core.routes()[1].group.as_deref(), Some("emailPassword"));
}
