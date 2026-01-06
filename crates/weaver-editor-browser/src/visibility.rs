//! DOM-based syntax visibility updates.
//!
//! This module applies visibility state to the DOM by toggling CSS classes
//! on syntax span elements. Works with the core `VisibilityState` calculation.
//!
//! # How it works
//!
//! 1. Core's `VisibilityState::calculate()` determines which syntax spans should be visible
//! 2. This module's `update_syntax_visibility()` applies that state to the DOM
//! 3. Elements with `data-syn-id` attributes get "hidden" class toggled
//!
//! # CSS Integration
//!
//! Your CSS should hide elements with the "hidden" class:
//! ```css
//! [data-syn-id].hidden {
//!     opacity: 0;
//!     /* or display: none, visibility: hidden, etc. */
//! }
//! ```

use weaver_editor_core::{ParagraphRender, Selection, SyntaxSpanInfo, VisibilityState};

/// Update syntax span visibility in the DOM based on cursor position.
///
/// Calculates which syntax spans should be visible using `VisibilityState::calculate()`,
/// then toggles the "hidden" class on matching DOM elements.
///
/// # Parameters
/// - `cursor_offset`: Current cursor position in characters
/// - `selection`: Optional text selection
/// - `syntax_spans`: All syntax spans from rendered paragraphs
/// - `paragraphs`: Rendered paragraph data for boundary detection
///
/// # DOM Requirements
/// Syntax span elements must have `data-syn-id` attributes matching `SyntaxSpanInfo.syn_id`.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn update_syntax_visibility(
    cursor_offset: usize,
    selection: Option<&Selection>,
    syntax_spans: &[SyntaxSpanInfo],
    paragraphs: &[ParagraphRender],
) {
    use wasm_bindgen::JsCast;

    let visibility = VisibilityState::calculate(cursor_offset, selection, syntax_spans, paragraphs);

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };

    // Single querySelectorAll instead of N individual queries.
    let Ok(node_list) = document.query_selector_all("[data-syn-id]") else {
        return;
    };

    for i in 0..node_list.length() {
        let Some(node) = node_list.item(i) else {
            continue;
        };

        let Some(element) = node.dyn_ref::<web_sys::Element>() else {
            continue;
        };

        let Some(syn_id) = element.get_attribute("data-syn-id") else {
            continue;
        };

        let class_list = element.class_list();
        if visibility.is_visible(&syn_id) {
            let _ = class_list.remove_1("hidden");
        } else {
            let _ = class_list.add_1("hidden");
        }
    }
}

/// No-op on non-WASM targets.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn update_syntax_visibility(
    _cursor_offset: usize,
    _selection: Option<&Selection>,
    _syntax_spans: &[SyntaxSpanInfo],
    _paragraphs: &[ParagraphRender],
) {
}
