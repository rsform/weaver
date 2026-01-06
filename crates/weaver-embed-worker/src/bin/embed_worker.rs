//! Entry point for the embed web worker.
//!
//! This binary is compiled separately and loaded by the main app
//! to fetch and cache AT Protocol embeds off the main thread.

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
fn main() {
    console_error_panic_hook::set_once();

    use gloo_worker::Registrable;
    use weaver_embed_worker::EmbedWorker;

    EmbedWorker::registrar().register();
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
fn main() {
    eprintln!("This binary is only meant to run as a WASM web worker");
}
