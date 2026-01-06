//! Editor actions and input types.
//!
//! Platform-agnostic definitions for editor operations. The `EditorAction` enum
//! represents semantic editing operations, while `InputType` represents the
//! semantic intent from input events (browser beforeinput, native input methods, etc.).

use smol_str::SmolStr;

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

impl From<std::ops::Range<usize>> for Range {
    fn from(r: std::ops::Range<usize>) -> Self {
        Self::new(r.start, r.end)
    }
}

impl From<Range> for std::ops::Range<usize> {
    fn from(r: Range) -> Self {
        r.start..r.end
    }
}

/// Semantic input types from input events.
///
/// These represent the semantic intent of an input operation, abstracted from
/// the platform-specific event source. Browser `beforeinput` events, native
/// input methods, and programmatic input can all produce these types.
///
/// Based on the W3C Input Events specification, but usable across platforms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputType {
    // === Insertion ===
    /// Insert typed text.
    InsertText,
    /// Insert text from IME composition.
    InsertCompositionText,
    /// Insert a line break (`<br>`, Shift+Enter).
    InsertLineBreak,
    /// Insert a paragraph break (Enter).
    InsertParagraph,
    /// Insert from paste operation.
    InsertFromPaste,
    /// Insert from drop operation.
    InsertFromDrop,
    /// Insert replacement text (e.g., spell check correction).
    InsertReplacementText,
    /// Insert from voice input or other source.
    InsertFromYank,
    /// Insert a horizontal rule.
    InsertHorizontalRule,
    /// Insert an ordered list.
    InsertOrderedList,
    /// Insert an unordered list.
    InsertUnorderedList,
    /// Insert a link.
    InsertLink,

    // === Deletion ===
    /// Delete content backward (Backspace).
    DeleteContentBackward,
    /// Delete content forward (Delete key).
    DeleteContentForward,
    /// Delete word backward (Ctrl/Alt+Backspace).
    DeleteWordBackward,
    /// Delete word forward (Ctrl/Alt+Delete).
    DeleteWordForward,
    /// Delete to soft line boundary backward.
    DeleteSoftLineBackward,
    /// Delete to soft line boundary forward.
    DeleteSoftLineForward,
    /// Delete to hard line boundary backward (Cmd+Backspace on Mac).
    DeleteHardLineBackward,
    /// Delete to hard line boundary forward (Cmd+Delete on Mac).
    DeleteHardLineForward,
    /// Delete by cut operation.
    DeleteByCut,
    /// Delete by drag operation.
    DeleteByDrag,
    /// Generic content deletion.
    DeleteContent,
    /// Delete entire word backward.
    DeleteEntireWordBackward,
    /// Delete entire word forward.
    DeleteEntireWordForward,

    // === History ===
    /// Undo.
    HistoryUndo,
    /// Redo.
    HistoryRedo,

    // === Formatting ===
    FormatBold,
    FormatItalic,
    FormatUnderline,
    FormatStrikethrough,
    FormatSuperscript,
    FormatSubscript,

    // === Unknown ===
    /// Unrecognized input type.
    Unknown(String),
}

impl InputType {
    /// Whether this input type is a deletion operation.
    pub fn is_deletion(&self) -> bool {
        matches!(
            self,
            Self::DeleteContentBackward
                | Self::DeleteContentForward
                | Self::DeleteWordBackward
                | Self::DeleteWordForward
                | Self::DeleteSoftLineBackward
                | Self::DeleteSoftLineForward
                | Self::DeleteHardLineBackward
                | Self::DeleteHardLineForward
                | Self::DeleteByCut
                | Self::DeleteByDrag
                | Self::DeleteContent
                | Self::DeleteEntireWordBackward
                | Self::DeleteEntireWordForward
        )
    }

    /// Whether this input type is an insertion operation.
    pub fn is_insertion(&self) -> bool {
        matches!(
            self,
            Self::InsertText
                | Self::InsertCompositionText
                | Self::InsertLineBreak
                | Self::InsertParagraph
                | Self::InsertFromPaste
                | Self::InsertFromDrop
                | Self::InsertReplacementText
                | Self::InsertFromYank
        )
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

    // === Navigation ===
    /// Move cursor to position.
    MoveCursor { offset: usize },

    /// Extend selection to position.
    ExtendSelection { offset: usize },
}

impl EditorAction {
    /// Update the range in actions that use one.
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
            other => other,
        }
    }
}

/// Key values for keyboard input.
///
/// Platform-agnostic key representation. Platform-specific code converts
/// from native key events to this enum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A character key.
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
    /// Create a character key.
    pub fn character(s: impl Into<SmolStr>) -> Self {
        Self::Character(s.into())
    }

    /// Check if this is a navigation key.
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
}

impl Modifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
        meta: false,
    };

    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
        meta: false,
    };

    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
        meta: false,
    };

    pub const META: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: true,
    };

    pub const CTRL_SHIFT: Self = Self {
        ctrl: true,
        alt: false,
        shift: true,
        meta: false,
    };

    pub const META_SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
        meta: true,
    };

    /// Get the primary modifier for the platform (Cmd on Mac, Ctrl elsewhere).
    pub fn primary(is_mac: bool) -> Self {
        if is_mac {
            Self::META
        } else {
            Self::CTRL
        }
    }

    /// Get the primary modifier + Shift for the platform.
    pub fn primary_shift(is_mac: bool) -> Self {
        if is_mac {
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

    pub fn primary(key: Key, is_mac: bool) -> Self {
        Self {
            key,
            modifiers: Modifiers::primary(is_mac),
        }
    }

    pub fn primary_shift(key: Key, is_mac: bool) -> Self {
        Self {
            key,
            modifiers: Modifiers::primary_shift(is_mac),
        }
    }
}

/// Result of handling a keydown event.
#[derive(Debug, Clone, PartialEq)]
pub enum KeydownResult {
    /// Event was handled, prevent default.
    Handled,
    /// Event was not a keybinding, let platform handle it.
    NotHandled,
    /// Event should be passed through (navigation, etc.).
    PassThrough,
}
