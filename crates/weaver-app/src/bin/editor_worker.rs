//! Entry point for the editor web worker.
//!
//! This binary is compiled separately and loaded by the main app
//! to handle CPU-intensive editor operations off the main thread.

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
fn main() {
    console_error_panic_hook::set_once();

    use gloo_worker::Registrable;
    use weaver_app::components::editor::EditorWorker;

    EditorWorker::registrar().register();
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
fn main() {
    eprintln!("This binary is only meant to run as a WASM web worker");
}
