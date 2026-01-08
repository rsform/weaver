//! Event handlers exposed to JavaScript.
//!
//! These handlers are called by the TypeScript view layer when DOM events fire.
//! The WASM side processes the events and updates state, returning whether
//! to preventDefault.

use wasm_bindgen::prelude::*;

use weaver_editor_browser::{
    BeforeInputContext, BeforeInputResult, handle_beforeinput, parse_browser_input_type, platform,
};
use weaver_editor_core::{
    EditorAction, EditorDocument, KeydownResult, Range, SnapDirection, execute_action,
    handle_keydown,
};

use crate::editor::JsEditor;

/// Result of handling an event.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    /// Event was handled, call preventDefault.
    Handled,
    /// Event should pass through to browser.
    PassThrough,
    /// Event was handled but needs async follow-up.
    HandledAsync,
}

impl From<BeforeInputResult> for EventResult {
    fn from(r: BeforeInputResult) -> Self {
        match r {
            BeforeInputResult::Handled => EventResult::Handled,
            BeforeInputResult::PassThrough => EventResult::PassThrough,
            BeforeInputResult::HandledAsync => EventResult::HandledAsync,
            BeforeInputResult::DeferredCheck { .. } => EventResult::PassThrough,
        }
    }
}

impl From<KeydownResult> for EventResult {
    fn from(r: KeydownResult) -> Self {
        match r {
            KeydownResult::Handled => EventResult::Handled,
            KeydownResult::PassThrough | KeydownResult::NotHandled => EventResult::PassThrough,
        }
    }
}

/// Target range for beforeinput event.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsTargetRange {
    pub start: usize,
    pub end: usize,
}

#[wasm_bindgen]
impl JsTargetRange {
    #[wasm_bindgen(constructor)]
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[wasm_bindgen]
impl JsEditor {
    // === Event handlers ===

    /// Handle beforeinput event.
    ///
    /// Returns whether to preventDefault.
    #[wasm_bindgen(js_name = handleBeforeInput)]
    pub fn handle_before_input(
        &mut self,
        input_type: &str,
        data: Option<String>,
        target_start: Option<usize>,
        target_end: Option<usize>,
        is_composing: bool,
    ) -> EventResult {
        let plat = platform::platform();
        let input_type_parsed = parse_browser_input_type(input_type);

        let target_range = match (target_start, target_end) {
            (Some(start), Some(end)) => Some(Range::new(start, end)),
            _ => None,
        };

        let ctx = BeforeInputContext {
            input_type: input_type_parsed,
            data,
            target_range,
            is_composing,
            platform: &plat,
        };

        let current_range = self.get_current_range();
        let result = handle_beforeinput(&mut self.doc, &ctx, current_range);

        // Handle deferred check (Android workaround) - for JS we just pass through
        // and let JS handle the timeout
        let event_result = EventResult::from(result);

        if event_result == EventResult::Handled {
            self.render_and_update_dom();
            self.notify_change();
        }

        event_result
    }

    /// Handle keydown event.
    ///
    /// Returns whether to preventDefault.
    #[wasm_bindgen(js_name = handleKeydown)]
    pub fn handle_keydown(
        &mut self,
        key: &str,
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    ) -> EventResult {
        // During IME composition, only handle Escape and modifier shortcuts
        if self.doc.composition().is_some() {
            if key == "Escape" {
                self.doc.set_composition(None);
                return EventResult::Handled;
            }
            if !ctrl && !alt && !meta {
                return EventResult::PassThrough;
            }
        }

        let combo = weaver_editor_core::KeyCombo {
            key: parse_key(key),
            modifiers: weaver_editor_core::Modifiers {
                ctrl,
                alt,
                shift,
                meta,
                hyper: false,
                super_: false,
            },
        };

        let cursor_offset = self.doc.cursor_offset();
        let selection = self.doc.selection();
        let range = selection
            .map(|s| Range::new(s.anchor.min(s.head), s.anchor.max(s.head)))
            .unwrap_or_else(|| Range::caret(cursor_offset));

        let keybindings = weaver_editor_core::KeybindingConfig::default();
        let result = handle_keydown(&mut self.doc, &keybindings, combo, range);

        let event_result = EventResult::from(result);

        if event_result == EventResult::Handled {
            self.render_and_update_dom();
            self.notify_change();
        }

        event_result
    }

    /// Handle keyup event for navigation keys.
    ///
    /// Syncs cursor from DOM after browser handles navigation.
    #[wasm_bindgen(js_name = handleKeyup)]
    pub fn handle_keyup(&mut self, key: &str) {
        let direction_hint = match key {
            "ArrowLeft" | "ArrowUp" => Some(SnapDirection::Backward),
            "ArrowRight" | "ArrowDown" => Some(SnapDirection::Forward),
            _ => None,
        };

        let is_navigation = matches!(
            key,
            "ArrowLeft"
                | "ArrowRight"
                | "ArrowUp"
                | "ArrowDown"
                | "Home"
                | "End"
                | "PageUp"
                | "PageDown"
        );

        if is_navigation {
            self.sync_cursor_with_hint(direction_hint);
        }
    }

    /// Sync cursor from DOM selection.
    ///
    /// Call this after click, select, or other events that change selection.
    #[wasm_bindgen(js_name = syncCursor)]
    pub fn sync_cursor(&mut self) {
        self.sync_cursor_with_hint(None);
    }

    /// Handle paste event.
    ///
    /// The text parameter is plain text from clipboard.
    #[wasm_bindgen(js_name = handlePaste)]
    pub fn handle_paste(&mut self, text: &str) {
        let cursor_offset = self.doc.cursor_offset();
        let selection = self.doc.selection();
        let range = selection
            .map(|s| Range::new(s.anchor.min(s.head), s.anchor.max(s.head)))
            .unwrap_or_else(|| Range::caret(cursor_offset));

        let action = EditorAction::Insert {
            text: text.to_string(),
            range,
        };
        execute_action(&mut self.doc, &action);

        self.render_and_update_dom();
        self.notify_change();
    }

    /// Handle cut event.
    ///
    /// Returns the text that was cut (for clipboard).
    #[wasm_bindgen(js_name = handleCut)]
    pub fn handle_cut(&mut self) -> Option<String> {
        let selection = self.doc.selection()?;
        let start = selection.anchor.min(selection.head);
        let end = selection.anchor.max(selection.head);

        if start == end {
            return None;
        }

        let text = self.doc.slice(start..end).map(|s| s.to_string())?;

        let action = EditorAction::Insert {
            text: String::new(),
            range: Range::new(start, end),
        };
        execute_action(&mut self.doc, &action);

        self.render_and_update_dom();
        self.notify_change();

        Some(text)
    }

    /// Handle copy event.
    ///
    /// Returns the text to copy (from selection).
    #[wasm_bindgen(js_name = handleCopy)]
    pub fn handle_copy(&self) -> Option<String> {
        let selection = self.doc.selection()?;
        let start = selection.anchor.min(selection.head);
        let end = selection.anchor.max(selection.head);

        if start == end {
            return None;
        }

        self.doc.slice(start..end).map(|s| s.to_string())
    }

    /// Handle blur event.
    ///
    /// Clears any in-progress IME composition.
    #[wasm_bindgen(js_name = handleBlur)]
    pub fn handle_blur(&mut self) {
        self.doc.set_composition(None);
    }

    /// Handle compositionstart event.
    #[wasm_bindgen(js_name = handleCompositionStart)]
    pub fn handle_composition_start(&mut self, data: Option<String>) {
        use weaver_editor_core::CompositionState;

        let cursor = self.doc.cursor_offset();
        self.doc.set_composition(Some(CompositionState {
            start_offset: cursor,
            text: data.unwrap_or_default(),
        }));
    }

    /// Handle compositionupdate event.
    #[wasm_bindgen(js_name = handleCompositionUpdate)]
    pub fn handle_composition_update(&mut self, data: Option<String>) {
        if let Some(mut comp) = self.doc.composition() {
            comp.text = data.unwrap_or_default();
            self.doc.set_composition(Some(comp));
        }
    }

    /// Handle compositionend event.
    #[wasm_bindgen(js_name = handleCompositionEnd)]
    pub fn handle_composition_end(&mut self, data: Option<String>) {
        // Get composition state before clearing
        let composition = self.doc.composition();
        self.doc.set_composition(None);

        // Insert the final composed text
        if let Some(comp) = composition {
            if let Some(text) = data {
                if !text.is_empty() {
                    let range = Range::new(
                        comp.start_offset,
                        comp.start_offset + comp.text.chars().count(),
                    );
                    let action = EditorAction::Insert { text, range };
                    execute_action(&mut self.doc, &action);

                    self.render_and_update_dom();
                    self.notify_change();
                }
            }
        }
    }

    /// Handle Android Enter key (workaround for keypress).
    #[wasm_bindgen(js_name = handleAndroidEnter)]
    pub fn handle_android_enter(&mut self) {
        let cursor_offset = self.doc.cursor_offset();
        let selection = self.doc.selection();
        let range = selection
            .map(|s| Range::new(s.anchor.min(s.head), s.anchor.max(s.head)))
            .unwrap_or_else(|| Range::caret(cursor_offset));

        let action = EditorAction::InsertParagraph { range };
        execute_action(&mut self.doc, &action);

        self.render_and_update_dom();
        self.notify_change();
    }
}

// Internal helpers
impl JsEditor {
    fn get_current_range(&self) -> Range {
        let cursor_offset = self.doc.cursor_offset();
        let selection = self.doc.selection();
        selection
            .map(|s| Range::new(s.anchor.min(s.head), s.anchor.max(s.head)))
            .unwrap_or_else(|| Range::caret(cursor_offset))
    }

    fn sync_cursor_with_hint(&mut self, direction_hint: Option<SnapDirection>) {
        use weaver_editor_browser::{CursorSyncResult, sync_cursor_from_dom_impl};

        let Some(ref editor_id) = self.editor_id else {
            return;
        };

        // Get sync result without closures to avoid borrow issues
        if let Some(result) = sync_cursor_from_dom_impl(editor_id, &self.paragraphs, direction_hint)
        {
            match result {
                CursorSyncResult::Cursor(offset) => {
                    self.doc.set_cursor_offset(offset);
                    self.doc.set_selection(None);
                }
                CursorSyncResult::Selection { anchor, head } => {
                    if anchor == head {
                        self.doc.set_cursor_offset(anchor);
                        self.doc.set_selection(None);
                    } else {
                        self.doc
                            .set_selection(Some(weaver_editor_core::Selection { anchor, head }));
                    }
                }
                CursorSyncResult::None => {}
            }
        }

        // Update syntax visibility after cursor sync
        let cursor_offset = self.doc.cursor_offset();
        let selection = self.doc.selection();
        let syntax_spans: Vec<_> = self
            .paragraphs
            .iter()
            .flat_map(|p| p.syntax_spans.iter().cloned())
            .collect();
        weaver_editor_browser::update_syntax_visibility(
            cursor_offset,
            selection.as_ref(),
            &syntax_spans,
            &self.paragraphs,
        );
    }
}

/// Parse a key string to the editor's Key enum.
fn parse_key(key: &str) -> weaver_editor_core::Key {
    use weaver_editor_core::Key;

    match key {
        "Enter" => Key::Enter,
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Tab" => Key::Tab,
        "Escape" => Key::Escape,
        "ArrowLeft" => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "ArrowUp" => Key::ArrowUp,
        "ArrowDown" => Key::ArrowDown,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        s if s.len() == 1 => Key::character(s),
        _ => Key::Unidentified,
    }
}
