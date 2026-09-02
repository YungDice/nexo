//! Meet&Greet, end to end.
//!
//! Four claims are checked here, and each is one the client cannot be trusted
//! to keep on the server's behalf:
//!
//!   * a submitted pin is never what gets stored;
//!   * the same account is coarsened identically every time, so saving twice
//!     discloses no more than saving once;
//!   * a blocked person is off the map in both directions;
//!   * an intro buys one message, and the second is refused by the server.
//!
//! Skips cleanly with no `DATABASE_URL`, like the rest of the suite.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use nexo_server::{AppState, TokenKeys, db, router};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
    MC4CAQAwBQYDK2VwBCIEIBD8O+mO1pxsOJPSKpso2043G54kPXsxDyl6dTJ6H5Io\n\
    -----END PRIVATE KEY-----\n";

async fn app() -> Option<axum::Router> {
    let _ = dotenvy::dotenv();
    let url = std::env::var("DATABASE_URL").ok()?;
    let db = db::create_pool(&url)
        .await
        .expect("connect to the database");
    Some(router(AppState {
        db,
        auth: Arc::new(TokenKeys::from_pem_bytes(TEST_KEY_PEM.as_bytes()).unwrap()),
        storage: None,
        fanout: Arc::new(nexo_server::stream::hub::LocalHub::new()),
        limits: Arc::new(nexo_server::limits::Limits::permissive()),
    }))
}

macro_rules! app_or_skip {
    () => {
        match app().await {
            Some(app) => app,
            None => {
                eprintln!("skipping: DATABASE_URL not set");
                return;
            }
        }
    };
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = match body {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

struct Party {
    token: String,
    handle: String,
}

async fn register(app: &axum::Router) -> Party {
    let handle = format!("m{}", Uuid::new_v4().simple())[..16].to_string();
    let (status, session) = call(
        app,
        "POST",
        "/v1/auth/register",
        None,
        Some(json!({
            "handle": handle,
            "display_name": "Meet Test",
            "pw_salt": Uuid::new_v4().simple().to_string(),
            "pw_verifier": "07".repeat(32),
            "identity_pubkey": format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
        })),
    )
    .await;
    // Never print the body: it carries a live access token and a refresh token.
    assert!(status.is_success(), "register returned {status}");
    Party {
        token: session["access_token"].as_str().unwrap().to_string(),
        handle,
    }
}

/// Accept the agreement and drop a pin. Returns what the server stored.
async fn place(app: &axum::Router, who: &Party, lat: f64, lon: f64) -> Value {
    let (status, _) = call(
        app,
        "POST",
        "/v1/meet/consent",
        Some(&who.token),
        Some(json!({ "version": 1 })),
    )
    .await;
    assert!(status.is_success(), "consent returned {status}");

    let (status, _) = call(
        app,
        "PUT",
        "/v1/meet/me",
        Some(&who.token),
        Some(json!({
            "lat": lat,
            "lon": lon,
            "char_config": { "topVariant": "hoodie" },
            "active": true,
        })),
    )
    .await;
    assert!(status.is_success(), "PUT /v1/meet/me returned {status}");

    let (status, mine) = call(app, "GET", "/v1/meet/me", Some(&who.token), None).await;
    assert_eq!(status, StatusCode::OK);
    mine
}

#[tokio::test]
async fn the_pin_that_is_stored_is_never_the_pin_that_was_sent() {
    let app = app_or_skip!();
    let alice = register(&app).await;

    let lat = 47.376_9;
    let lon = 8.541_7;
    let stored = place(&app, &alice, lat, lon).await;

    assert_ne!(
        stored["lat"].as_f64().unwrap(),
        lat,
        "the submitted latitude must not survive the write"
    );
    assert_ne!(stored["lon"].as_f64().unwrap(), lon);

    // Still roughly where they said, or the feature does not work.
    assert!((stored["lat"].as_f64().unwrap() - lat).abs() < 0.5);
    assert!((stored["lon"].as_f64().unwrap() - lon).abs() < 0.5);
}

/// Saving twice must disclose no more than saving once.
#[tokio::test]
async fn moving_a_pin_back_lands_it_in_the_same_place_again() {
    let app = app_or_skip!();
    let alice = register(&app).await;

    let first = place(&app, &alice, 47.376_9, 8.541_7).await;
    let elsewhere = place(&app, &alice, 12.0, 34.0).await;
    let back = place(&app, &alice, 47.376_9, 8.541_7).await;

    assert_ne!(first["lat"], elsewhere["lat"], "the pin should have moved");
    assert_eq!(
        first["lat"], back["lat"],
        "a re-rolled jitter would let an observer average the offsets away"
    );
    assert_eq!(first["lon"], back["lon"]);
}

#[tokio::test]
async fn a_blocked_person_is_off_the_map_in_both_directions() {
    let app = app_or_skip!();
    let alice = register(&app).await;
    let mal = register(&app).await;

    place(&app, &alice, 47.0, 8.0).await;
    place(&app, &mal, 47.1, 8.1).await;

    let on_map = |pins: &Value, handle: &str| -> bool {
        pins.as_array()
            .unwrap()
            .iter()
            .any(|p| p["handle"] == handle)
    };

    let (_, pins) = call(&app, "GET", "/v1/meet/pins", Some(&alice.token), None).await;
    assert!(on_map(&pins, &mal.handle), "they start out visible");

    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/blocks/{}", mal.handle),
        Some(&alice.token),
        None,
    )
    .await;
    assert!(status.is_success(), "block returned {status}");

    let (_, pins) = call(&app, "GET", "/v1/meet/pins", Some(&alice.token), None).await;
    assert!(!on_map(&pins, &mal.handle), "the blocked pin must be gone");

    // And the other way, which is the half a client-side filter would miss.
    let (_, pins) = call(&app, "GET", "/v1/meet/pins", Some(&mal.token), None).await;
    assert!(
        !on_map(&pins, &alice.handle),
        "blocking removes both pins, not one"
    );
}

#[tokio::test]
async fn the_map_needs_the_agreement_first() {
    let app = app_or_skip!();
    let alice = register(&app).await;

    let (status, body) = call(
        &app,
        "PUT",
        "/v1/meet/me",
        Some(&alice.token),
        Some(json!({ "lat": 1.0, "lon": 2.0, "char_config": {} })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "consent_required");
}

/// A conversation, and the request that makes it an intro.
async fn intro(app: &axum::Router, from: &Party, to: &Party) -> Uuid {
    let conversation_id = Uuid::new_v4();
    let (status, _) = call(
        app,
        "POST",
        "/v1/conversations",
        Some(&from.token),
        Some(json!({
            "conversation_id": conversation_id,
            "members": [to.handle],
        })),
    )
    .await;
    assert!(status.is_success(), "create conversation returned {status}");

    let (status, _) = call(
        app,
        "POST",
        "/v1/meet/requests",
        Some(&from.token),
        Some(json!({ "handle": to.handle, "conversation_id": conversation_id })),
    )
    .await;
    assert!(status.is_success(), "open request returned {status}");
    conversation_id
}

async fn send(app: &axum::Router, who: &Party, conversation: Uuid, text: &str) -> StatusCode {
    let (status, _) = call(
        app,
        "POST",
        &format!("/v1/conversations/{conversation}/send"),
        Some(&who.token),
        Some(json!({
            "ciphertext": hex_of(text),
            "epoch": 0,
            "is_commit": false,
            "client_msg_id": Uuid::new_v4(),
        })),
    )
    .await;
    status
}

fn hex_of(text: &str) -> String {
    text.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// The rule the whole feature rests on, and the one a client cannot keep.
#[tokio::test]
async fn an_intro_buys_exactly_one_message() {
    let app = app_or_skip!();
    let alice = register(&app).await;
    let bob = register(&app).await;
    place(&app, &alice, 47.0, 8.0).await;
    place(&app, &bob, 48.0, 9.0).await;

    let conversation = intro(&app, &alice, &bob).await;

    assert!(
        send(&app, &alice, conversation, "hello").await.is_success(),
        "the first message is the whole point"
    );
    assert_eq!(
        send(&app, &alice, conversation, "hello again").await,
        StatusCode::FORBIDDEN,
        "the second must be refused by the server, not by the app"
    );
}

#[tokio::test]
async fn answering_an_intro_lifts_the_cap() {
    let app = app_or_skip!();
    let alice = register(&app).await;
    let bob = register(&app).await;
    place(&app, &alice, 47.0, 8.0).await;
    place(&app, &bob, 48.0, 9.0).await;

    let conversation = intro(&app, &alice, &bob).await;
    assert!(send(&app, &alice, conversation, "hello").await.is_success());

    let (_, inbox) = call(&app, "GET", "/v1/meet/requests", Some(&bob.token), None).await;
    let id = inbox.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/meet/requests/{id}/accept"),
        Some(&bob.token),
        None,
    )
    .await;
    assert!(status.is_success(), "accept returned {status}");

    assert!(
        send(&app, &alice, conversation, "thanks")
            .await
            .is_success(),
        "once answered it is an ordinary conversation"
    );
}

/// A retry and a second attempt look identical from the handler; only the
/// UNIQUE constraint can tell them apart, and it must not produce two.
#[tokio::test]
async fn a_repeated_intro_is_refused_rather_than_duplicated() {
    let app = app_or_skip!();
    let alice = register(&app).await;
    let bob = register(&app).await;
    place(&app, &alice, 47.0, 8.0).await;
    place(&app, &bob, 48.0, 9.0).await;

    let first = intro(&app, &alice, &bob).await;

    let second = Uuid::new_v4();
    let (status, _) = call(
        &app,
        "POST",
        "/v1/conversations",
        Some(&alice.token),
        Some(json!({ "conversation_id": second, "members": [bob.handle] })),
    )
    .await;
    assert!(status.is_success());

    let (status, _) = call(
        &app,
        "POST",
        "/v1/meet/requests",
        Some(&alice.token),
        Some(json!({ "handle": bob.handle, "conversation_id": second })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a second intro is refused");

    let (_, inbox) = call(&app, "GET", "/v1/meet/requests", Some(&bob.token), None).await;
    let waiting = inbox.as_array().unwrap();
    assert_eq!(waiting.len(), 1, "one intro, however many times it is sent");
    assert_eq!(waiting[0]["conversation_id"], json!(first));
}

#[tokio::test]
async fn leaving_the_map_keeps_the_character() {
    let app = app_or_skip!();
    let alice = register(&app).await;
    place(&app, &alice, 47.0, 8.0).await;

    let (status, _) = call(&app, "DELETE", "/v1/meet/me", Some(&alice.token), None).await;
    assert!(status.is_success());

    let (status, _) = call(&app, "GET", "/v1/meet/me", Some(&alice.token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "off the map");

    // Coming back is one tap, and the character is still there.
    let (status, _) = call(
        &app,
        "PUT",
        "/v1/meet/me",
        Some(&alice.token),
        Some(json!({ "active": true })),
    )
    .await;
    assert!(status.is_success(), "returning returned {status}");

    let (status, mine) = call(&app, "GET", "/v1/meet/me", Some(&alice.token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mine["char_config"]["topVariant"], "hoodie");
}
