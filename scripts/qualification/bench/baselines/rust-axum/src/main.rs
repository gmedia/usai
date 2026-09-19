//! Rust-native comparator (the attribution control): axum + deadpool-postgres,
//! the same routes, the same validation rules (serde structs plus the explicit
//! bounds the Zod schemas carry), the same SQL, the same error envelope. No
//! execution world, no ownership ledger: what remains is HTTP + PostgreSQL.
//! PORT, DATABASE_URL, POOL_MAX.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use deadpool_postgres::{Config, Pool, Runtime};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

struct App {
    pool: Pool,
    counter: AtomicU64,
}

fn reply(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}
fn err(status: StatusCode, code: &str, message: &str) -> Response {
    reply(status, json!({ "error": { "code": code, "message": message } }))
}
fn invalid(slot: &str, path: &str, message: &str) -> Response {
    reply(
        StatusCode::BAD_REQUEST,
        json!({ "error": { "code": "validation_failed", "message": format!("{slot} failed validation"), "details": { "slot": slot, "issues": [{ "message": message, "path": path }] } } }),
    )
}

#[derive(Serialize, Deserialize)]
struct User {
    id: i32,
    name: String,
    email: String,
}

fn parse_id(raw: &str) -> Result<i32, Response> {
    match raw.parse::<i64>() {
        Ok(n) if (1..=2_147_483_647).contains(&n) => Ok(n as i32),
        _ => Err(invalid("params", "/id", "Invalid input: expected number, received NaN")),
    }
}

async fn health() -> Response {
    reply(StatusCode::OK, json!({ "ok": true }))
}

async fn hello(Path(name): Path<String>) -> Response {
    if name.is_empty() || name.chars().count() > 40 {
        return invalid("params", "/name", "Too big: expected string to have <=40 characters");
    }
    reply(StatusCode::OK, json!({ "hello": name }))
}

#[derive(Deserialize)]
struct Item {
    sku: String,
    quantity: i64,
    #[serde(rename = "unitCents")]
    unit_cents: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Quote {
    customer: String,
    email: String,
    currency: String,
    country: String,
    #[serde(default)]
    coupon_code: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default = "normal")]
    priority: String,
    #[serde(default)]
    gift: bool,
    requested_date: String,
    reference: uuid::Uuid,
    tags: Vec<String>,
    items: Vec<Item>,
}
fn normal() -> String {
    "normal".into()
}
static EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$").unwrap());
static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap());

async fn quote(body: Result<Json<Quote>, axum::extract::rejection::JsonRejection>) -> Response {
    let Json(b) = match body {
        Ok(b) => b,
        Err(e) => return invalid("body", "", &e.body_text()),
    };
    // The bounds the Zod schema enforces, by hand (serde only checks shape).
    if b.customer.is_empty() || b.customer.len() > 200 { return invalid("body", "/customer", "length"); }
    if !EMAIL.is_match(&b.email) { return invalid("body", "/email", "Invalid email address"); }
    if !["USD", "EUR", "IDR"].contains(&b.currency.as_str()) { return invalid("body", "/currency", "Invalid option"); }
    if b.country.chars().count() != 2 { return invalid("body", "/country", "length"); }
    if b.coupon_code.as_ref().is_some_and(|c| c.len() > 20) { return invalid("body", "/couponCode", "length"); }
    if b.notes.as_ref().is_some_and(|c| c.len() > 500) { return invalid("body", "/notes", "length"); }
    if !["low", "normal", "high"].contains(&b.priority.as_str()) { return invalid("body", "/priority", "Invalid option"); }
    if !DATE.is_match(&b.requested_date) { return invalid("body", "/requestedDate", "Invalid string"); }
    if b.tags.len() > 10 || b.tags.iter().any(|t| t.len() > 16) { return invalid("body", "/tags", "length"); }
    if b.items.is_empty() || b.items.len() > 50 { return invalid("body", "/items", "length"); }
    for (i, item) in b.items.iter().enumerate() {
        if item.sku.is_empty() || item.sku.len() > 32 { return invalid("body", &format!("/items/{i}/sku"), "length"); }
        if !(1..=1000).contains(&item.quantity) { return invalid("body", &format!("/items/{i}/quantity"), "range"); }
        if !(0..=10_000_000).contains(&item.unit_cents) { return invalid("body", &format!("/items/{i}/unitCents"), "range"); }
    }
    let _ = b.gift;
    let subtotal: i64 = b.items.iter().map(|i| i.quantity * i.unit_cents).sum();
    let rate = match b.country.as_str() { "ID" => 11, "DE" => 19, _ => 0 };
    let tax = ((subtotal * rate) as f64 / 100.0).round() as i64;
    reply(
        StatusCode::CREATED,
        json!({ "reference": b.reference, "currency": b.currency, "subtotalCents": subtotal, "taxCents": tax, "totalCents": subtotal + tax, "lines": b.items.len(), "priority": b.priority }),
    )
}

async fn get_user(State(app): State<Arc<App>>, Path(raw): Path<String>) -> Response {
    let id = match parse_id(&raw) { Ok(id) => id, Err(r) => return r };
    let client = match app.pool.get().await { Ok(c) => c, Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "unavailable", "pool") };
    let stmt = client.prepare_cached("select id, name, email from users where id = $1").await.unwrap();
    match client.query_opt(&stmt, &[&id]).await {
        Ok(Some(row)) => reply(StatusCode::OK, serde_json::to_value(User { id: row.get(0), name: row.get(1), email: row.get(2) }).unwrap()),
        Ok(None) => err(StatusCode::NOT_FOUND, "not_found", "user_not_found"),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error"),
    }
}

#[derive(Deserialize)]
struct NewUser {
    name: String,
    email: String,
}
async fn create_user(State(app): State<Arc<App>>, body: Result<Json<NewUser>, axum::extract::rejection::JsonRejection>) -> Response {
    let Json(b) = match body { Ok(b) => b, Err(e) => return invalid("body", "", &e.body_text()) };
    if b.name.is_empty() || b.name.len() > 200 { return invalid("body", "/name", "length"); }
    if !EMAIL.is_match(&b.email) { return invalid("body", "/email", "Invalid email address"); }
    let client = match app.pool.get().await { Ok(c) => c, Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "unavailable", "pool") };
    let stmt = client.prepare_cached("insert into users (name, email) values ($1, $2) returning id, name, email").await.unwrap();
    match client.query_one(&stmt, &[&b.name, &b.email]).await {
        Ok(row) => reply(StatusCode::CREATED, serde_json::to_value(User { id: row.get(0), name: row.get(1), email: row.get(2) }).unwrap()),
        Err(e) if e.code().map(|c| c.code()) == Some("23505") => err(StatusCode::CONFLICT, "conflict", "email_taken"),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error"),
    }
}

async fn pay(State(app): State<Arc<App>>, Path(raw): Path<String>) -> Response {
    let id = match parse_id(&raw) { Ok(id) => id, Err(r) => return r };
    let mut client = match app.pool.get().await { Ok(c) => c, Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "unavailable", "pool") };
    let tx = match client.transaction().await { Ok(t) => t, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error") };
    let sel = tx.prepare_cached("select id, total_cents, paid from orders where id = $1 for update").await.unwrap();
    let order = match tx.query_opt(&sel, &[&id]).await {
        Ok(Some(row)) => row,
        Ok(None) => return err(StatusCode::NOT_FOUND, "not_found", "order_not_found"),
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error"),
    };
    let (oid, total, paid): (i32, i32, bool) = (order.get(0), order.get(1), order.get(2));
    if paid { return err(StatusCode::CONFLICT, "conflict", "already_paid"); }
    let upd = tx.prepare_cached("update orders set paid = true where id = $1").await.unwrap();
    if tx.execute(&upd, &[&oid]).await.is_err() { return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error"); }
    let ins = tx.prepare_cached("insert into payments (order_id, amount_cents) values ($1, $2) returning id").await.unwrap();
    let pid: i32 = match tx.query_one(&ins, &[&oid, &total]).await { Ok(r) => r.get(0), Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error") };
    if tx.commit().await.is_err() { return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error"); }
    reply(StatusCode::OK, json!({ "orderId": oid, "paymentId": pid, "amountCents": total, "paid": true }))
}

async fn me(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(key) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) else {
        return err(StatusCode::UNAUTHORIZED, "unauthorized", "missing x-api-key header");
    };
    let client = match app.pool.get().await { Ok(c) => c, Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "unavailable", "pool") };
    let k = client.prepare_cached("select user_id from api_keys where key = $1").await.unwrap();
    let user_id: i32 = match client.query_opt(&k, &[&key]).await {
        Ok(Some(row)) => row.get(0),
        _ => return err(StatusCode::UNAUTHORIZED, "unauthorized", "unknown_key"),
    };
    let stmt = client.prepare_cached("select id, name, email from users where id = $1").await.unwrap();
    match client.query_opt(&stmt, &[&user_id]).await {
        Ok(Some(row)) => reply(StatusCode::OK, serde_json::to_value(User { id: row.get(0), name: row.get(1), email: row.get(2) }).unwrap()),
        Ok(None) => err(StatusCode::NOT_FOUND, "not_found", "user_not_found"),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error"),
    }
}

async fn counter(State(app): State<Arc<App>>) -> Response {
    reply(StatusCode::OK, json!({ "count": app.counter.fetch_add(1, Ordering::Relaxed) + 1 }))
}

#[derive(Deserialize)]
struct Slow {
    ms: Option<String>,
}
async fn slow(Query(q): Query<Slow>) -> Response {
    let ms = match q.ms.as_deref().map(str::parse::<i64>) {
        None => 1000,
        Some(Ok(n)) if (0..=30_000).contains(&n) => n,
        _ => return invalid("query", "/ms", "range"),
    };
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
    reply(StatusCode::OK, json!({ "slept": ms }))
}

async fn not_found() -> Response {
    err(StatusCode::NOT_FOUND, "route_not_found", "no route matches")
}

#[tokio::main]
async fn main() {
    let mut cfg = Config::new();
    cfg.url = Some(std::env::var("DATABASE_URL").expect("DATABASE_URL"));
    cfg.pool = Some(deadpool_postgres::PoolConfig::new(std::env::var("POOL_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(64)));
    let pool = cfg.create_pool(Some(Runtime::Tokio1), tokio_postgres::NoTls).expect("pool");
    let app = Arc::new(App { pool, counter: AtomicU64::new(0) });
    let router = Router::new()
        .route("/health", get(health))
        .route("/hello/{name}", get(hello))
        .route("/orders/quote", post(quote))
        .route("/users/{id}", get(get_user))
        .route("/users", post(create_user))
        .route("/orders/{id}/pay", post(pay))
        .route("/me", get(me))
        .route("/counter", get(counter))
        .route("/slow", get(slow))
        .fallback(not_found)
        .with_state(app);
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3004);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    axum::serve(listener, router).await.unwrap();
}
