//! EditorAction conversion for JavaScript.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;
use weaver_editor_core::{EditorAction, FormatAction, Range};

/// JavaScript-friendly editor action.
///
/// Mirrors EditorAction from core but with JS-compatible types.
/// Also includes FormatAction variants for extended formatting.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum JsEditorAction {
    // Text insertion
    Insert { text: String, start: usize, end: usize },
    InsertLineBreak { start: usize, end: usize },
    InsertParagraph { start: usize, end: usize },

    // Deletion
    DeleteBackward { start: usize, end: usize },
    DeleteForward { start: usize, end: usize },
    DeleteWordBackward { start: usize, end: usize },
    DeleteWordForward { start: usize, end: usize },
    DeleteToLineStart { start: usize, end: usize },
    DeleteToLineEnd { start: usize, end: usize },
    DeleteSoftLineBackward { start: usize, end: usize },
    DeleteSoftLineForward { start: usize, end: usize },

    // History
    Undo,
    Redo,

    // Inline formatting (EditorAction variants)
    ToggleBold,
    ToggleItalic,
    ToggleCode,
    ToggleStrikethrough,
    InsertLink,

    // Extended formatting (FormatAction variants)
    InsertImage,
    InsertHeading { level: u8 },
    ToggleBulletList,
    ToggleNumberedList,
    ToggleQuote,

    // Clipboard
    Cut,
    Copy,
    Paste { start: usize, end: usize },
    CopyAsHtml,

    // Selection
    SelectAll,

    // Cursor
    MoveCursor { offset: usize },
    ExtendSelection { offset: usize },
}

/// Result of converting JsEditorAction.
pub enum ActionKind {
    /// Standard EditorAction.
    Editor(EditorAction),
    /// FormatAction (needs apply_formatting).
    Format(FormatAction),
}

impl JsEditorAction {
    /// Convert to ActionKind (either EditorAction or FormatAction).
    pub fn to_action_kind(&self) -> ActionKind {
        match self {
            // Text insertion
            Self::Insert { text, start, end } => ActionKind::Editor(EditorAction::Insert {
                text: text.clone(),
                range: Range::new(*start, *end),
            }),
            Self::InsertLineBreak { start, end } => ActionKind::Editor(EditorAction::InsertLineBreak {
                range: Range::new(*start, *end),
            }),
            Self::InsertParagraph { start, end } => ActionKind::Editor(EditorAction::InsertParagraph {
                range: Range::new(*start, *end),
            }),

            // Deletion
            Self::DeleteBackward { start, end } => ActionKind::Editor(EditorAction::DeleteBackward {
                range: Range::new(*start, *end),
            }),
            Self::DeleteForward { start, end } => ActionKind::Editor(EditorAction::DeleteForward {
                range: Range::new(*start, *end),
            }),
            Self::DeleteWordBackward { start, end } => ActionKind::Editor(EditorAction::DeleteWordBackward {
                range: Range::new(*start, *end),
            }),
            Self::DeleteWordForward { start, end } => ActionKind::Editor(EditorAction::DeleteWordForward {
                range: Range::new(*start, *end),
            }),
            Self::DeleteToLineStart { start, end } => ActionKind::Editor(EditorAction::DeleteToLineStart {
                range: Range::new(*start, *end),
            }),
            Self::DeleteToLineEnd { start, end } => ActionKind::Editor(EditorAction::DeleteToLineEnd {
                range: Range::new(*start, *end),
            }),
            Self::DeleteSoftLineBackward { start, end } => ActionKind::Editor(EditorAction::DeleteSoftLineBackward {
                range: Range::new(*start, *end),
            }),
            Self::DeleteSoftLineForward { start, end } => ActionKind::Editor(EditorAction::DeleteSoftLineForward {
                range: Range::new(*start, *end),
            }),

            // History
            Self::Undo => ActionKind::Editor(EditorAction::Undo),
            Self::Redo => ActionKind::Editor(EditorAction::Redo),

            // Inline formatting (EditorAction)
            Self::ToggleBold => ActionKind::Editor(EditorAction::ToggleBold),
            Self::ToggleItalic => ActionKind::Editor(EditorAction::ToggleItalic),
            Self::ToggleCode => ActionKind::Editor(EditorAction::ToggleCode),
            Self::ToggleStrikethrough => ActionKind::Editor(EditorAction::ToggleStrikethrough),
            Self::InsertLink => ActionKind::Editor(EditorAction::InsertLink),

            // Extended formatting (FormatAction)
            Self::InsertImage => ActionKind::Format(FormatAction::Image),
            Self::InsertHeading { level } => ActionKind::Format(FormatAction::Heading(*level)),
            Self::ToggleBulletList => ActionKind::Format(FormatAction::BulletList),
            Self::ToggleNumberedList => ActionKind::Format(FormatAction::NumberedList),
            Self::ToggleQuote => ActionKind::Format(FormatAction::Quote),

            // Clipboard
            Self::Cut => ActionKind::Editor(EditorAction::Cut),
            Self::Copy => ActionKind::Editor(EditorAction::Copy),
            Self::Paste { start, end } => ActionKind::Editor(EditorAction::Paste {
                range: Range::new(*start, *end),
            }),
            Self::CopyAsHtml => ActionKind::Editor(EditorAction::CopyAsHtml),

            // Selection
            Self::SelectAll => ActionKind::Editor(EditorAction::SelectAll),

            // Cursor
            Self::MoveCursor { offset } => ActionKind::Editor(EditorAction::MoveCursor { offset: *offset }),
            Self::ExtendSelection { offset } => ActionKind::Editor(EditorAction::ExtendSelection { offset: *offset }),
        }
    }
}

/// Parse a JsValue into JsEditorAction.
pub fn parse_action(value: JsValue) -> Result<JsEditorAction, JsError> {
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| JsError::new(&format!("Invalid action: {}", e)))
}
