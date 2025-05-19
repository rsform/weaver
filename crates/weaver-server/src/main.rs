//! Weaver server
//!
//! This crate is a lightweight HTTP server which can serve a notebook.
//! It will auto-reload

use axum::{Router, response::Html, routing::get};

use tokio::net::TcpListener;

#[tokio::main]
async fn main() {}

async fn handler() -> Html<&'static str> {
    Html("<h1>Hello, World!</h1>")
}
