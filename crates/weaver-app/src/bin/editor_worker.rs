//! Entry point for the editor web worker.
//!
//! This binary is compiled separately and loaded by the main app
//! to handle CPU-intensive editor operations off the main thread.

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
fn main() {
    console_error_panic_hook::set_once();
    use tracing::Level;
    use tracing::subscriber::set_global_default;
    use tracing_subscriber::Registry;
    use tracing_subscriber::filter::EnvFilter;
    use tracing_subscriber::layer::SubscriberExt;

    let console_level = if cfg!(debug_assertions) {
        Level::DEBUG
    } else {
        Level::DEBUG
    };

    let wasm_layer = tracing_wasm::WASMLayer::new(
        tracing_wasm::WASMLayerConfigBuilder::new()
            .set_max_level(console_level)
            .build(),
    );

    // Filter out noisy crates
    let filter = EnvFilter::new(
        "debug,loro_internal=warn,jacquard_identity=info,jacquard_common=info,iroh=info",
    );

    let reg = Registry::default().with(filter).with(wasm_layer);

    let _ = set_global_default(reg);

    use gloo_worker::Registrable;
    use weaver_app::components::editor::EditorReactor;

    EditorReactor::registrar().register();
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
fn main() {
    eprintln!("This binary is only meant to run as a WASM web worker");
}
