//! Editor actions and keybinding system.
//!
//! This module defines all editor operations as an enum, providing a clean
//! abstraction layer between input events and document mutations. This enables:
//!
//! - Configurable keybindings (user can remap shortcuts)
//! - Platform-specific defaults (Cmd vs Ctrl, etc.)
//! - Consistent action handling regardless of input source
//! - Potential for command palette, macros, etc.

use std::collections::HashMap;

use dioxus::prelude::*;
use jacquard::smol_str::SmolStr;

use super::document::EditorDocument;
use super::platform::Platform;

/// A range in the document, measured in character offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

impl Range {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn caret(offset: usize) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    pub fn is_caret(&self) -> bool {
        self.start == self.end
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Normalize range so start <= end.
    pub fn normalize(self) -> Self {
        if self.start <= self.end {
            self
        } else {
            Self {
                start: self.end,
                end: self.start,
            }
        }
    }
}

/// All possible editor actions.
///
/// These represent semantic operations on the document, decoupled from
/// how they're triggered (keyboard, mouse, touch, voice, etc.).
#[derive(Debug, Clone, PartialEq)]
pub enum EditorAction {
    // === Text Insertion ===
    /// Insert text at the given range (replacing any selected content).
    Insert { text: String, range: Range },

    /// Insert a soft line break (Shift+Enter, `<br>` equivalent).
    InsertLineBreak { range: Range },

    /// Insert a paragraph break (Enter).
    InsertParagraph { range: Range },

    // === Deletion ===
    /// Delete content backward (Backspace).
    DeleteBackward { range: Range },

    /// Delete content forward (Delete key).
    DeleteForward { range: Range },

    /// Delete word backward (Ctrl/Alt+Backspace).
    DeleteWordBackward { range: Range },

    /// Delete word forward (Ctrl/Alt+Delete).
    DeleteWordForward { range: Range },

    /// Delete to start of line (Cmd+Backspace on Mac).
    DeleteToLineStart { range: Range },

    /// Delete to end of line (Cmd+Delete on Mac).
    DeleteToLineEnd { range: Range },

    /// Delete to start of soft line (visual line in wrapped text).
    DeleteSoftLineBackward { range: Range },

    /// Delete to end of soft line.
    DeleteSoftLineForward { range: Range },

    // === History ===
    /// Undo the last change.
    Undo,

    /// Redo the last undone change.
    Redo,

    // === Formatting ===
    /// Toggle bold on selection.
    ToggleBold,

    /// Toggle italic on selection.
    ToggleItalic,

    /// Toggle inline code on selection.
    ToggleCode,

    /// Toggle strikethrough on selection.
    ToggleStrikethrough,

    /// Insert/wrap with link.
    InsertLink,

    // === Clipboard ===
    /// Cut selection to clipboard.
    Cut,

    /// Copy selection to clipboard.
    Copy,

    /// Paste from clipboard at range.
    Paste { range: Range },

    /// Copy selection as rendered HTML.
    CopyAsHtml,

    // === Selection ===
    /// Select all content.
    SelectAll,

    // === Navigation (for command palette / programmatic use) ===
    /// Move cursor to position.
    MoveCursor { offset: usize },

    /// Extend selection to position.
    ExtendSelection { offset: usize },
}

impl EditorAction {
    /// Update the range in actions that use one.
    /// Actions without a range are returned unchanged.
    pub fn with_range(self, range: Range) -> Self {
        match self {
            Self::Insert { text, .. } => Self::Insert { text, range },
            Self::InsertLineBreak { .. } => Self::InsertLineBreak { range },
            Self::InsertParagraph { .. } => Self::InsertParagraph { range },
            Self::DeleteBackward { .. } => Self::DeleteBackward { range },
            Self::DeleteForward { .. } => Self::DeleteForward { range },
            Self::DeleteWordBackward { .. } => Self::DeleteWordBackward { range },
            Self::DeleteWordForward { .. } => Self::DeleteWordForward { range },
            Self::DeleteToLineStart { .. } => Self::DeleteToLineStart { range },
            Self::DeleteToLineEnd { .. } => Self::DeleteToLineEnd { range },
            Self::DeleteSoftLineBackward { .. } => Self::DeleteSoftLineBackward { range },
            Self::DeleteSoftLineForward { .. } => Self::DeleteSoftLineForward { range },
            Self::Paste { .. } => Self::Paste { range },
            // Actions without range stay unchanged
            other => other,
        }
    }
}

/// Key values for keyboard input.
///
/// Mirrors the keyboard-types crate's Key enum structure. Character keys use
/// SmolStr to efficiently handle both single characters and composed sequences
/// (from dead keys, IME, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A character key. The string corresponds to the character typed,
    /// taking into account locale, modifiers, and keyboard mapping.
    /// May be multiple characters for composed sequences.
    Character(SmolStr),

    /// Unknown/unidentified key.
    Unidentified,

    // === Whitespace / editing ===
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Space,
    Insert,
    Clear,

    // === Navigation ===
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,

    // === Modifiers ===
    Alt,
    AltGraph,
    CapsLock,
    Control,
    Fn,
    FnLock,
    Meta,
    NumLock,
    ScrollLock,
    Shift,
    Symbol,
    SymbolLock,
    Hyper,
    Super,

    // === Function keys ===
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,

    // === UI keys ===
    ContextMenu,
    PrintScreen,
    Pause,
    Help,

    // === Clipboard / editing commands ===
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    Find,
    Select,

    // === Media keys ===
    MediaPlayPause,
    MediaStop,
    MediaTrackNext,
    MediaTrackPrevious,
    AudioVolumeDown,
    AudioVolumeUp,
    AudioVolumeMute,

    // === IME / composition ===
    Compose,
    Convert,
    NonConvert,
    Dead,

    // === CJK IME keys ===
    HangulMode,
    HanjaMode,
    JunjaMode,
    Eisu,
    Hankaku,
    Hiragana,
    HiraganaKatakana,
    KanaMode,
    KanjiMode,
    Katakana,
    Romaji,
    Zenkaku,
    ZenkakuHankaku,
}

impl Key {
    /// Create a character key from a string.
    pub fn character(s: impl Into<SmolStr>) -> Self {
        Self::Character(s.into())
    }

    /// Convert from a dioxus keyboard_types::Key.
    pub fn from_keyboard_types(key: dioxus::prelude::keyboard_types::Key) -> Self {
        use dioxus::prelude::keyboard_types::Key as KT;

        match key {
            KT::Character(s) => Self::Character(s.as_str().into()),
            KT::Unidentified => Self::Unidentified,

            // Whitespace / editing
            KT::Backspace => Self::Backspace,
            KT::Delete => Self::Delete,
            KT::Enter => Self::Enter,
            KT::Tab => Self::Tab,
            KT::Escape => Self::Escape,
            KT::Insert => Self::Insert,
            KT::Clear => Self::Clear,

            // Navigation
            KT::ArrowLeft => Self::ArrowLeft,
            KT::ArrowRight => Self::ArrowRight,
            KT::ArrowUp => Self::ArrowUp,
            KT::ArrowDown => Self::ArrowDown,
            KT::Home => Self::Home,
            KT::End => Self::End,
            KT::PageUp => Self::PageUp,
            KT::PageDown => Self::PageDown,

            // Modifiers
            KT::Alt => Self::Alt,
            KT::AltGraph => Self::AltGraph,
            KT::CapsLock => Self::CapsLock,
            KT::Control => Self::Control,
            KT::Fn => Self::Fn,
            KT::FnLock => Self::FnLock,
            KT::Meta => Self::Meta,
            KT::NumLock => Self::NumLock,
            KT::ScrollLock => Self::ScrollLock,
            KT::Shift => Self::Shift,
            KT::Symbol => Self::Symbol,
            KT::SymbolLock => Self::SymbolLock,
            KT::Hyper => Self::Hyper,
            KT::Super => Self::Super,

            // Function keys
            KT::F1 => Self::F1,
            KT::F2 => Self::F2,
            KT::F3 => Self::F3,
            KT::F4 => Self::F4,
            KT::F5 => Self::F5,
            KT::F6 => Self::F6,
            KT::F7 => Self::F7,
            KT::F8 => Self::F8,
            KT::F9 => Self::F9,
            KT::F10 => Self::F10,
            KT::F11 => Self::F11,
            KT::F12 => Self::F12,
            KT::F13 => Self::F13,
            KT::F14 => Self::F14,
            KT::F15 => Self::F15,
            KT::F16 => Self::F16,
            KT::F17 => Self::F17,
            KT::F18 => Self::F18,
            KT::F19 => Self::F19,
            KT::F20 => Self::F20,

            // UI keys
            KT::ContextMenu => Self::ContextMenu,
            KT::PrintScreen => Self::PrintScreen,
            KT::Pause => Self::Pause,
            KT::Help => Self::Help,

            // Clipboard / editing commands
            KT::Copy => Self::Copy,
            KT::Cut => Self::Cut,
            KT::Paste => Self::Paste,
            KT::Undo => Self::Undo,
            KT::Redo => Self::Redo,
            KT::Find => Self::Find,
            KT::Select => Self::Select,

            // Media keys
            KT::MediaPlayPause => Self::MediaPlayPause,
            KT::MediaStop => Self::MediaStop,
            KT::MediaTrackNext => Self::MediaTrackNext,
            KT::MediaTrackPrevious => Self::MediaTrackPrevious,
            KT::AudioVolumeDown => Self::AudioVolumeDown,
            KT::AudioVolumeUp => Self::AudioVolumeUp,
            KT::AudioVolumeMute => Self::AudioVolumeMute,

            // IME / composition
            KT::Compose => Self::Compose,
            KT::Convert => Self::Convert,
            KT::NonConvert => Self::NonConvert,
            KT::Dead => Self::Dead,

            // CJK IME keys
            KT::HangulMode => Self::HangulMode,
            KT::HanjaMode => Self::HanjaMode,
            KT::JunjaMode => Self::JunjaMode,
            KT::Eisu => Self::Eisu,
            KT::Hankaku => Self::Hankaku,
            KT::Hiragana => Self::Hiragana,
            KT::HiraganaKatakana => Self::HiraganaKatakana,
            KT::KanaMode => Self::KanaMode,
            KT::KanjiMode => Self::KanjiMode,
            KT::Katakana => Self::Katakana,
            KT::Romaji => Self::Romaji,
            KT::Zenkaku => Self::Zenkaku,
            KT::ZenkakuHankaku => Self::ZenkakuHankaku,

            // Everything else falls through to Unidentified
            _ => Self::Unidentified,
        }
    }

    /// Check if this is a navigation key that should pass through to browser.
    pub fn is_navigation(&self) -> bool {
        matches!(
            self,
            Self::ArrowLeft
                | Self::ArrowRight
                | Self::ArrowUp
                | Self::ArrowDown
                | Self::Home
                | Self::End
                | Self::PageUp
                | Self::PageDown
        )
    }

    /// Check if this is a modifier key.
    pub fn is_modifier(&self) -> bool {
        matches!(
            self,
            Self::Alt
                | Self::AltGraph
                | Self::CapsLock
                | Self::Control
                | Self::Fn
                | Self::FnLock
                | Self::Meta
                | Self::NumLock
                | Self::ScrollLock
                | Self::Shift
                | Self::Symbol
                | Self::SymbolLock
                | Self::Hyper
                | Self::Super
        )
    }
}

/// Modifier key state for a key combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub hyper: bool,
    pub super_: bool, // `super` is a keyword
}

impl Modifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
        hyper: false,
        super_: false,
    };
    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
        meta: false,
        hyper: false,
        super_: false,
    };
    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
        meta: false,
        hyper: false,
        super_: false,
    };
    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
        meta: false,
        hyper: false,
        super_: false,
    };
    pub const META: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: true,
        hyper: false,
        super_: false,
    };
    pub const HYPER: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
        hyper: true,
        super_: false,
    };
    pub const SUPER: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
        hyper: false,
        super_: true,
    };
    pub const CTRL_SHIFT: Self = Self {
        ctrl: true,
        alt: false,
        shift: true,
        meta: false,
        hyper: false,
        super_: false,
    };
    pub const META_SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
        meta: true,
        hyper: false,
        super_: false,
    };

    /// Get the platform's primary modifier (Cmd on Mac, Ctrl elsewhere).
    pub fn cmd_or_ctrl(platform: &Platform) -> Self {
        if platform.mac { Self::META } else { Self::CTRL }
    }

    /// Get the platform's primary modifier + Shift.
    pub fn cmd_or_ctrl_shift(platform: &Platform) -> Self {
        if platform.mac {
            Self::META_SHIFT
        } else {
            Self::CTRL_SHIFT
        }
    }
}

/// A key combination for triggering an action.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl KeyCombo {
    pub fn new(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::NONE,
        }
    }

    pub fn with_modifiers(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    pub fn ctrl(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::CTRL,
        }
    }

    pub fn meta(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::META,
        }
    }

    pub fn shift(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::SHIFT,
        }
    }

    pub fn cmd_or_ctrl(key: Key, platform: &Platform) -> Self {
        Self {
            key,
            modifiers: Modifiers::cmd_or_ctrl(platform),
        }
    }

    pub fn cmd_or_ctrl_shift(key: Key, platform: &Platform) -> Self {
        Self {
            key,
            modifiers: Modifiers::cmd_or_ctrl_shift(platform),
        }
    }

    /// Create a KeyCombo from a dioxus keyboard event.
    pub fn from_keyboard_event(event: &dioxus::events::KeyboardData) -> Self {
        let key = Key::from_keyboard_types(event.key());
        let modifiers = Modifiers {
            ctrl: event.modifiers().ctrl(),
            alt: event.modifiers().alt(),
            shift: event.modifiers().shift(),
            meta: event.modifiers().meta(),
            // dioxus doesn't expose hyper/super separately, they typically map to meta
            hyper: false,
            super_: false,
        };
        Self { key, modifiers }
    }
}

/// Keybinding configuration for the editor.
///
/// Uses a HashMap for O(1) keybinding lookup.
#[derive(Debug, Clone)]
pub struct KeybindingConfig {
    bindings: HashMap<KeyCombo, EditorAction>,
}

impl KeybindingConfig {
    /// Create default keybindings for the given platform.
    pub fn default_for_platform(platform: &Platform) -> Self {
        let mut bindings = HashMap::new();

        // === Formatting ===
        bindings.insert(
            KeyCombo::cmd_or_ctrl(Key::character("b"), platform),
            EditorAction::ToggleBold,
        );
        bindings.insert(
            KeyCombo::cmd_or_ctrl(Key::character("i"), platform),
            EditorAction::ToggleItalic,
        );
        bindings.insert(
            KeyCombo::cmd_or_ctrl(Key::character("e"), platform),
            EditorAction::CopyAsHtml,
        );

        // === History ===
        bindings.insert(
            KeyCombo::cmd_or_ctrl(Key::character("z"), platform),
            EditorAction::Undo,
        );

        // Redo: Cmd+Shift+Z on Mac, Ctrl+Y or Ctrl+Shift+Z elsewhere
        if platform.mac {
            bindings.insert(
                KeyCombo::cmd_or_ctrl_shift(Key::character("Z"), platform),
                EditorAction::Redo,
            );
        } else {
            bindings.insert(KeyCombo::ctrl(Key::character("y")), EditorAction::Redo);
            bindings.insert(
                KeyCombo::with_modifiers(Key::character("Z"), Modifiers::CTRL_SHIFT),
                EditorAction::Redo,
            );
        }

        // === Selection ===
        bindings.insert(
            KeyCombo::cmd_or_ctrl(Key::character("a"), platform),
            EditorAction::SelectAll,
        );

        // === Line deletion ===
        if platform.mac {
            bindings.insert(
                KeyCombo::meta(Key::Backspace),
                EditorAction::DeleteToLineStart {
                    range: Range::caret(0),
                },
            );
            bindings.insert(
                KeyCombo::meta(Key::Delete),
                EditorAction::DeleteToLineEnd {
                    range: Range::caret(0),
                },
            );
        }

        // === Enter behaviour ===
        // Enter = soft break (single newline)
        bindings.insert(
            KeyCombo::new(Key::Enter),
            EditorAction::InsertLineBreak {
                range: Range::caret(0),
            },
        );
        // Shift+Enter = paragraph break (double newline)
        bindings.insert(
            KeyCombo::shift(Key::Enter),
            EditorAction::InsertParagraph {
                range: Range::caret(0),
            },
        );

        // === Dedicated editing keys (for custom keyboards, etc.) ===
        bindings.insert(KeyCombo::new(Key::Undo), EditorAction::Undo);
        bindings.insert(KeyCombo::new(Key::Redo), EditorAction::Redo);
        bindings.insert(KeyCombo::new(Key::Copy), EditorAction::Copy);
        bindings.insert(KeyCombo::new(Key::Cut), EditorAction::Cut);
        bindings.insert(
            KeyCombo::new(Key::Paste),
            EditorAction::Paste {
                range: Range::caret(0),
            },
        );
        bindings.insert(KeyCombo::new(Key::Select), EditorAction::SelectAll);

        Self { bindings }
    }

    /// Look up an action for the given key combo, with the current range applied.
    pub fn lookup(&self, combo: KeyCombo, range: Range) -> Option<EditorAction> {
        self.bindings.get(&combo).cloned().map(|a| a.with_range(range))
    }

    /// Look up an action for the given key and modifiers, with the current range applied.
    pub fn lookup_key(&self, key: Key, modifiers: Modifiers, range: Range) -> Option<EditorAction> {
        self.lookup(KeyCombo::with_modifiers(key, modifiers), range)
    }

    /// Add or replace a keybinding.
    pub fn bind(&mut self, combo: KeyCombo, action: EditorAction) {
        self.bindings.insert(combo, action);
    }

    /// Remove a keybinding.
    pub fn unbind(&mut self, combo: KeyCombo) {
        self.bindings.remove(&combo);
    }
}

/// Execute an editor action on a document.
///
/// This is the central dispatch point for all editor operations.
/// Returns true if the action was handled and the document was modified.
pub fn execute_action(doc: &mut EditorDocument, action: &EditorAction) -> bool {
    use super::formatting::{self, FormatAction};
    use super::input::{
        detect_list_context, find_line_end, find_line_start, get_char_at, is_list_item_empty,
    };
    use super::offset_map::SnapDirection;

    match action {
        EditorAction::Insert { text, range } => {
            let range = range.normalize();
            if range.is_caret() {
                // Simple insert
                let offset = range.start;

                // Clean up any preceding zero-width chars
                let mut delete_start = offset;
                while delete_start > 0 {
                    match get_char_at(doc.loro_text(), delete_start - 1) {
                        Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
                        _ => break,
                    }
                }

                let zw_count = offset - delete_start;
                if zw_count > 0 {
                    let _ = doc.replace_tracked(delete_start, zw_count, text);
                    doc.cursor.write().offset = delete_start + text.chars().count();
                } else if offset == doc.len_chars() {
                    let _ = doc.push_tracked(text);
                    doc.cursor.write().offset = offset + text.chars().count();
                } else {
                    let _ = doc.insert_tracked(offset, text);
                    doc.cursor.write().offset = offset + text.chars().count();
                }
            } else {
                // Replace range
                let _ = doc.replace_tracked(range.start, range.len(), text);
                doc.cursor.write().offset = range.start + text.chars().count();
            }
            doc.selection.set(None);
            true
        }

        EditorAction::InsertLineBreak { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            if !range.is_caret() {
                let _ = doc.remove_tracked(range.start, range.len());
            }

            let mut offset = range.start;

            // Check if we're right after a soft break (newline + zero-width char).
            // If so, convert to paragraph break by replacing the zero-width char
            // with a newline.
            let mut is_double_enter = false;
            if offset >= 2 {
                let prev_char = get_char_at(doc.loro_text(), offset - 1);
                let prev_prev_char = get_char_at(doc.loro_text(), offset - 2);
                if prev_char == Some('\u{200C}') && prev_prev_char == Some('\n') {
                    // Replace zero-width char with newline
                    let _ = doc.replace_tracked(offset - 1, 1, "\n");
                    doc.cursor.write().offset = offset;
                    is_double_enter = true;
                }
            }

            if !is_double_enter {
                // Normal soft break: insert newline + zero-width char for cursor positioning.
                // The renderer emits <br> for soft breaks, so we don't need
                // trailing spaces for visual line breaks.
                let _ = doc.insert_tracked(offset, "\n\u{200C}");
                doc.cursor.write().offset = offset + 2;
            }

            doc.selection.set(None);
            true
        }

        EditorAction::InsertParagraph { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            if !range.is_caret() {
                let _ = doc.remove_tracked(range.start, range.len());
            }

            let cursor_offset = range.start;

            // Check for list context
            if let Some(ctx) = detect_list_context(doc.loro_text(), cursor_offset) {
                if is_list_item_empty(doc.loro_text(), cursor_offset, &ctx) {
                    // Empty item - exit list
                    let line_start = find_line_start(doc.loro_text(), cursor_offset);
                    let line_end = find_line_end(doc.loro_text(), cursor_offset);
                    let delete_end = (line_end + 1).min(doc.len_chars());

                    let _ = doc.replace_tracked(
                        line_start,
                        delete_end.saturating_sub(line_start),
                        "\n\n\u{200C}\n",
                    );
                    doc.cursor.write().offset = line_start + 2;
                } else {
                    // Continue list
                    let continuation = match ctx {
                        super::input::ListContext::Unordered { indent, marker } => {
                            format!("\n{}{} ", indent, marker)
                        }
                        super::input::ListContext::Ordered { indent, number } => {
                            format!("\n{}{}. ", indent, number + 1)
                        }
                    };
                    let len = continuation.chars().count();
                    let _ = doc.insert_tracked(cursor_offset, &continuation);
                    doc.cursor.write().offset = cursor_offset + len;
                }
            } else {
                // Normal paragraph break
                let _ = doc.insert_tracked(cursor_offset, "\n\n");
                doc.cursor.write().offset = cursor_offset + 2;
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteBackward { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Backward));

            if !range.is_caret() {
                // Delete selection
                let _ = doc.remove_tracked(range.start, range.len());
                doc.cursor.write().offset = range.start;
            } else if range.start > 0 {
                let cursor_offset = range.start;
                let prev_char = get_char_at(doc.loro_text(), cursor_offset - 1);

                if prev_char == Some('\n') {
                    // Deleting a newline - handle paragraph merging
                    let newline_pos = cursor_offset - 1;
                    let mut delete_start = newline_pos;
                    let mut delete_end = cursor_offset;

                    // Check for empty paragraph (double newline)
                    if newline_pos > 0 {
                        if get_char_at(doc.loro_text(), newline_pos - 1) == Some('\n') {
                            delete_start = newline_pos - 1;
                        }
                    }

                    // Check for trailing zero-width char
                    if let Some(ch) = get_char_at(doc.loro_text(), delete_end) {
                        if ch == '\u{200C}' || ch == '\u{200B}' {
                            delete_end += 1;
                        }
                    }

                    // Scan backwards through zero-width chars
                    while delete_start > 0 {
                        match get_char_at(doc.loro_text(), delete_start - 1) {
                            Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
                            Some('\n') | _ => break,
                        }
                    }

                    let _ =
                        doc.remove_tracked(delete_start, delete_end.saturating_sub(delete_start));
                    doc.cursor.write().offset = delete_start;
                } else {
                    // Normal single char delete
                    let _ = doc.remove_tracked(cursor_offset - 1, 1);
                    doc.cursor.write().offset = cursor_offset - 1;
                }
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteForward { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            if !range.is_caret() {
                let _ = doc.remove_tracked(range.start, range.len());
                doc.cursor.write().offset = range.start;
            } else if range.start < doc.len_chars() {
                let _ = doc.remove_tracked(range.start, 1);
                // Cursor stays at same position
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteWordBackward { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Backward));

            if !range.is_caret() {
                let _ = doc.remove_tracked(range.start, range.len());
                doc.cursor.write().offset = range.start;
            } else {
                // Find word boundary backwards
                let cursor = range.start;
                let word_start = find_word_boundary_backward(doc, cursor);
                if word_start < cursor {
                    let _ = doc.remove_tracked(word_start, cursor - word_start);
                    doc.cursor.write().offset = word_start;
                }
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteWordForward { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            if !range.is_caret() {
                let _ = doc.remove_tracked(range.start, range.len());
                doc.cursor.write().offset = range.start;
            } else {
                // Find word boundary forward
                let cursor = range.start;
                let word_end = find_word_boundary_forward(doc, cursor);
                if word_end > cursor {
                    let _ = doc.remove_tracked(cursor, word_end - cursor);
                }
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteToLineStart { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Backward));

            let cursor = if range.is_caret() {
                range.start
            } else {
                range.start
            };
            let line_start = find_line_start(doc.loro_text(), cursor);

            if line_start < cursor {
                let _ = doc.remove_tracked(line_start, cursor - line_start);
                doc.cursor.write().offset = line_start;
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteToLineEnd { range } => {
            let range = range.normalize();
            doc.pending_snap.set(Some(SnapDirection::Forward));

            let cursor = if range.is_caret() {
                range.start
            } else {
                range.end
            };
            let line_end = find_line_end(doc.loro_text(), cursor);

            if cursor < line_end {
                let _ = doc.remove_tracked(cursor, line_end - cursor);
            }

            doc.selection.set(None);
            true
        }

        EditorAction::DeleteSoftLineBackward { range } => {
            // For now, treat same as DeleteToLineStart
            // TODO: Handle visual line wrapping if needed
            execute_action(doc, &EditorAction::DeleteToLineStart { range: *range })
        }

        EditorAction::DeleteSoftLineForward { range } => {
            // For now, treat same as DeleteToLineEnd
            execute_action(doc, &EditorAction::DeleteToLineEnd { range: *range })
        }

        EditorAction::Undo => {
            if let Ok(true) = doc.undo() {
                let max = doc.len_chars();
                doc.cursor.with_mut(|c| c.offset = c.offset.min(max));
                doc.selection.set(None);
                true
            } else {
                false
            }
        }

        EditorAction::Redo => {
            if let Ok(true) = doc.redo() {
                let max = doc.len_chars();
                doc.cursor.with_mut(|c| c.offset = c.offset.min(max));
                doc.selection.set(None);
                true
            } else {
                false
            }
        }

        EditorAction::ToggleBold => {
            formatting::apply_formatting(doc, FormatAction::Bold);
            true
        }

        EditorAction::ToggleItalic => {
            formatting::apply_formatting(doc, FormatAction::Italic);
            true
        }

        EditorAction::ToggleCode => {
            formatting::apply_formatting(doc, FormatAction::Code);
            true
        }

        EditorAction::ToggleStrikethrough => {
            formatting::apply_formatting(doc, FormatAction::Strikethrough);
            true
        }

        EditorAction::InsertLink => {
            formatting::apply_formatting(doc, FormatAction::Link);
            true
        }

        EditorAction::Cut => {
            // Handled separately via clipboard events
            false
        }

        EditorAction::Copy => {
            // Handled separately via clipboard events
            false
        }

        EditorAction::Paste { range: _ } => {
            // Handled separately via clipboard events (needs async clipboard access)
            false
        }

        EditorAction::CopyAsHtml => {
            // Handled in component with async clipboard access
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            {
                if let Some(sel) = *doc.selection.read() {
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    if start != end {
                        if let Some(markdown) = doc.slice(start, end) {
                            let clean_md = markdown.replace('\u{200C}', "").replace('\u{200B}', "");
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Err(e) = super::input::copy_as_html(&clean_md).await {
                                    tracing::warn!("[COPY HTML] Failed: {:?}", e);
                                }
                            });
                            return true;
                        }
                    }
                }
            }
            false
        }

        EditorAction::SelectAll => {
            let len = doc.len_chars();
            doc.selection.set(Some(super::document::Selection {
                anchor: 0,
                head: len,
            }));
            doc.cursor.write().offset = len;
            true
        }

        EditorAction::MoveCursor { offset } => {
            let offset = (*offset).min(doc.len_chars());
            doc.cursor.write().offset = offset;
            doc.selection.set(None);
            true
        }

        EditorAction::ExtendSelection { offset } => {
            let offset = (*offset).min(doc.len_chars());
            let current_sel = *doc.selection.read();
            let anchor = current_sel
                .map(|s| s.anchor)
                .unwrap_or(doc.cursor.read().offset);
            doc.selection.set(Some(super::document::Selection {
                anchor,
                head: offset,
            }));
            doc.cursor.write().offset = offset;
            true
        }
    }
}

/// Find word boundary backward from cursor.
fn find_word_boundary_backward(doc: &EditorDocument, cursor: usize) -> usize {
    use super::input::get_char_at;

    if cursor == 0 {
        return 0;
    }

    let mut pos = cursor;

    // Skip any whitespace/punctuation immediately before cursor
    while pos > 0 {
        match get_char_at(doc.loro_text(), pos - 1) {
            Some(c) if c.is_alphanumeric() || c == '_' => break,
            Some(_) => pos -= 1,
            None => break,
        }
    }

    // Skip the word characters
    while pos > 0 {
        match get_char_at(doc.loro_text(), pos - 1) {
            Some(c) if c.is_alphanumeric() || c == '_' => pos -= 1,
            _ => break,
        }
    }

    pos
}

/// Find word boundary forward from cursor.
fn find_word_boundary_forward(doc: &EditorDocument, cursor: usize) -> usize {
    use super::input::get_char_at;

    let len = doc.len_chars();
    if cursor >= len {
        return len;
    }

    let mut pos = cursor;

    // Skip word characters first
    while pos < len {
        match get_char_at(doc.loro_text(), pos) {
            Some(c) if c.is_alphanumeric() || c == '_' => pos += 1,
            _ => break,
        }
    }

    // Then skip whitespace/punctuation
    while pos < len {
        match get_char_at(doc.loro_text(), pos) {
            Some(c) if c.is_alphanumeric() || c == '_' => break,
            Some(_) => pos += 1,
            None => break,
        }
    }

    pos
}

/// Result of handling a keydown event.
#[derive(Debug, Clone, PartialEq)]
pub enum KeydownResult {
    /// Event was handled, prevent default.
    Handled,
    /// Event was not a keybinding, let browser/beforeinput handle it.
    NotHandled,
    /// Event should be passed through (navigation, etc.).
    PassThrough,
}

/// Handle a keydown event using the keybinding configuration.
///
/// This handles keyboard shortcuts only. Text input and deletion
/// are handled by beforeinput. Navigation (arrows, etc.) is passed
/// through to the browser.
///
/// # Arguments
/// * `doc` - The editor document
/// * `config` - Keybinding configuration
/// * `combo` - The key combination from the keyboard event
/// * `range` - Current cursor position / selection range
///
/// # Returns
/// Whether the event was handled.
pub fn handle_keydown_with_bindings(
    doc: &mut EditorDocument,
    config: &KeybindingConfig,
    combo: KeyCombo,
    range: Range,
) -> KeydownResult {
    // Look up keybinding (range is applied by lookup)
    if let Some(action) = config.lookup(combo.clone(), range) {
        execute_action(doc, &action);
        return KeydownResult::Handled;
    }

    // No keybinding matched - check if this is navigation or content
    if combo.key.is_navigation() {
        return KeydownResult::PassThrough;
    }

    // Modifier-only keypresses should pass through
    if combo.key.is_modifier() {
        return KeydownResult::PassThrough;
    }

    // Content keys (typing, backspace, etc.) - let beforeinput handle
    KeydownResult::NotHandled
}
