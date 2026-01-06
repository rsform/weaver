//! Editor actions and keybinding system.
//!
//! This module re-exports core types and provides Dioxus-specific conversions.
//! Action execution delegates entirely to `weaver_editor_core`.

use dioxus::prelude::ModifiersInteraction;

use super::document::SignalEditorDocument;
use weaver_editor_browser::Platform;

// Re-export core types.
pub use weaver_editor_core::{
    EditorAction, Key, KeyCombo, KeybindingConfig, KeydownResult, Modifiers, Range,
};

// === Dioxus conversion helpers ===

/// Convert a dioxus keyboard_types::Key to our Key type.
pub fn key_from_dioxus(key: dioxus::prelude::keyboard_types::Key) -> Key {
    use dioxus::prelude::keyboard_types::Key as KT;

    match key {
        KT::Character(s) => Key::character(s.as_str()),
        KT::Unidentified => Key::Unidentified,
        KT::Backspace => Key::Backspace,
        KT::Delete => Key::Delete,
        KT::Enter => Key::Enter,
        KT::Tab => Key::Tab,
        KT::Escape => Key::Escape,
        KT::Insert => Key::Insert,
        KT::Clear => Key::Clear,
        KT::ArrowLeft => Key::ArrowLeft,
        KT::ArrowRight => Key::ArrowRight,
        KT::ArrowUp => Key::ArrowUp,
        KT::ArrowDown => Key::ArrowDown,
        KT::Home => Key::Home,
        KT::End => Key::End,
        KT::PageUp => Key::PageUp,
        KT::PageDown => Key::PageDown,
        KT::Alt => Key::Alt,
        KT::AltGraph => Key::AltGraph,
        KT::CapsLock => Key::CapsLock,
        KT::Control => Key::Control,
        KT::Fn => Key::Fn,
        KT::FnLock => Key::FnLock,
        KT::Meta => Key::Meta,
        KT::NumLock => Key::NumLock,
        KT::ScrollLock => Key::ScrollLock,
        KT::Shift => Key::Shift,
        KT::Symbol => Key::Symbol,
        KT::SymbolLock => Key::SymbolLock,
        KT::Hyper => Key::Hyper,
        KT::Super => Key::Super,
        KT::F1 => Key::F1,
        KT::F2 => Key::F2,
        KT::F3 => Key::F3,
        KT::F4 => Key::F4,
        KT::F5 => Key::F5,
        KT::F6 => Key::F6,
        KT::F7 => Key::F7,
        KT::F8 => Key::F8,
        KT::F9 => Key::F9,
        KT::F10 => Key::F10,
        KT::F11 => Key::F11,
        KT::F12 => Key::F12,
        KT::F13 => Key::F13,
        KT::F14 => Key::F14,
        KT::F15 => Key::F15,
        KT::F16 => Key::F16,
        KT::F17 => Key::F17,
        KT::F18 => Key::F18,
        KT::F19 => Key::F19,
        KT::F20 => Key::F20,
        KT::ContextMenu => Key::ContextMenu,
        KT::PrintScreen => Key::PrintScreen,
        KT::Pause => Key::Pause,
        KT::Help => Key::Help,
        KT::Copy => Key::Copy,
        KT::Cut => Key::Cut,
        KT::Paste => Key::Paste,
        KT::Undo => Key::Undo,
        KT::Redo => Key::Redo,
        KT::Find => Key::Find,
        KT::Select => Key::Select,
        KT::MediaPlayPause => Key::MediaPlayPause,
        KT::MediaStop => Key::MediaStop,
        KT::MediaTrackNext => Key::MediaTrackNext,
        KT::MediaTrackPrevious => Key::MediaTrackPrevious,
        KT::AudioVolumeDown => Key::AudioVolumeDown,
        KT::AudioVolumeUp => Key::AudioVolumeUp,
        KT::AudioVolumeMute => Key::AudioVolumeMute,
        KT::Compose => Key::Compose,
        KT::Convert => Key::Convert,
        KT::NonConvert => Key::NonConvert,
        KT::Dead => Key::Dead,
        KT::HangulMode => Key::HangulMode,
        KT::HanjaMode => Key::HanjaMode,
        KT::JunjaMode => Key::JunjaMode,
        KT::Eisu => Key::Eisu,
        KT::Hankaku => Key::Hankaku,
        KT::Hiragana => Key::Hiragana,
        KT::HiraganaKatakana => Key::HiraganaKatakana,
        KT::KanaMode => Key::KanaMode,
        KT::KanjiMode => Key::KanjiMode,
        KT::Katakana => Key::Katakana,
        KT::Romaji => Key::Romaji,
        KT::Zenkaku => Key::Zenkaku,
        KT::ZenkakuHankaku => Key::ZenkakuHankaku,
        _ => Key::Unidentified,
    }
}

/// Create a KeyCombo from a dioxus keyboard event.
pub fn keycombo_from_dioxus_event(event: &dioxus::events::KeyboardData) -> KeyCombo {
    let key = key_from_dioxus(event.key());
    let modifiers = Modifiers {
        ctrl: event.modifiers().ctrl(),
        alt: event.modifiers().alt(),
        shift: event.modifiers().shift(),
        meta: event.modifiers().meta(),
        hyper: false,
        super_: false,
    };
    KeyCombo::with_modifiers(key, modifiers)
}

/// Create a default KeybindingConfig for the given platform.
pub fn default_keybindings(platform: &Platform) -> KeybindingConfig {
    KeybindingConfig::default_for_platform(platform.mac)
}

/// Execute an editor action on a document with browser clipboard support.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn execute_action(doc: &mut SignalEditorDocument, action: &EditorAction) -> bool {
    use weaver_editor_browser::BrowserClipboard;
    use weaver_editor_core::execute_action_with_clipboard;

    let clipboard = BrowserClipboard::empty();
    execute_action_with_clipboard(doc, action, &clipboard)
}

/// Execute an editor action on a document (non-browser fallback).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn execute_action(doc: &mut SignalEditorDocument, action: &EditorAction) -> bool {
    weaver_editor_core::execute_action(doc, action)
}

/// Handle a keydown event with browser clipboard support.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn handle_keydown_with_bindings(
    doc: &mut SignalEditorDocument,
    config: &KeybindingConfig,
    combo: KeyCombo,
    range: Range,
) -> KeydownResult {
    use weaver_editor_browser::BrowserClipboard;
    use weaver_editor_core::handle_keydown_with_clipboard;

    let clipboard = BrowserClipboard::empty();
    handle_keydown_with_clipboard(doc, config, combo, range, &clipboard)
}

/// Handle a keydown event (non-browser fallback).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn handle_keydown_with_bindings(
    doc: &mut SignalEditorDocument,
    config: &KeybindingConfig,
    combo: KeyCombo,
    range: Range,
) -> KeydownResult {
    weaver_editor_core::handle_keydown(doc, config, combo, range)
}
