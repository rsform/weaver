//! WASM browser tests for weaver-editor-browser.
//!
//! Run with: `wasm-pack test --headless --firefox` or `--chrome`

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use weaver_editor_browser::{
    BeforeInputContext, BeforeInputResult, InputType, Platform, Range, handle_beforeinput,
    parse_browser_input_type, platform,
};
use weaver_editor_core::{EditorDocument, EditorRope, PlainEditor, UndoableBuffer};

type TestEditor = PlainEditor<UndoableBuffer<EditorRope>>;

fn make_editor(content: &str) -> TestEditor {
    let rope = EditorRope::from_str(content);
    let buf = UndoableBuffer::new(rope, 100);
    PlainEditor::new(buf)
}

fn test_platform() -> Platform {
    Platform {
        ios: false,
        mac: false,
        android: false,
        chrome: false,
        safari: false,
        gecko: false,
        webkit_version: None,
        chrome_version: None,
        mobile: false,
    }
}

// === InputType parsing tests ===

#[wasm_bindgen_test]
fn test_parse_insert_text() {
    assert_eq!(
        parse_browser_input_type("insertText"),
        InputType::InsertText
    );
}

#[wasm_bindgen_test]
fn test_parse_delete_backward() {
    assert_eq!(
        parse_browser_input_type("deleteContentBackward"),
        InputType::DeleteContentBackward
    );
}

#[wasm_bindgen_test]
fn test_parse_unknown() {
    match parse_browser_input_type("unknownType") {
        InputType::Unknown(s) => assert_eq!(s, "unknownType"),
        _ => panic!("Expected Unknown variant"),
    }
}

// === Platform detection tests ===

#[wasm_bindgen_test]
fn test_platform_detection() {
    let plat = platform();
    // Just verify it returns something without panicking.
    // Actual values depend on the browser running the test.
    let _ = plat.mac;
    let _ = plat.safari;
    let _ = plat.chrome;
    let _ = plat.gecko;
    let _ = plat.android;
    let _ = plat.ios;
    let _ = plat.mobile;
}

// === BeforeInput handler tests ===

#[wasm_bindgen_test]
fn test_handle_insert_text() {
    let mut editor = make_editor("hello");
    editor.set_cursor_offset(5);
    let plat = test_platform();

    let ctx = BeforeInputContext {
        input_type: InputType::InsertText,
        data: Some(" world".to_string()),
        target_range: None,
        is_composing: false,
        platform: &plat,
    };

    let result = handle_beforeinput(&mut editor, &ctx, Range::caret(5));
    assert!(matches!(result, BeforeInputResult::Handled));
    assert_eq!(editor.content_string(), "hello world");
}

#[wasm_bindgen_test]
fn test_handle_delete_backward() {
    let mut editor = make_editor("hello");
    editor.set_cursor_offset(5);
    let plat = test_platform();

    let ctx = BeforeInputContext {
        input_type: InputType::DeleteContentBackward,
        data: None,
        target_range: None,
        is_composing: false,
        platform: &plat,
    };

    let result = handle_beforeinput(&mut editor, &ctx, Range::caret(5));
    assert!(matches!(result, BeforeInputResult::Handled));
    assert_eq!(editor.content_string(), "hell");
}

#[wasm_bindgen_test]
fn test_handle_composition_passthrough() {
    let mut editor = make_editor("hello");
    let plat = test_platform();

    let ctx = BeforeInputContext {
        input_type: InputType::InsertText,
        data: Some("x".to_string()),
        target_range: None,
        is_composing: true, // During composition
        platform: &plat,
    };

    let result = handle_beforeinput(&mut editor, &ctx, Range::caret(5));
    assert!(matches!(result, BeforeInputResult::PassThrough));
    // Document unchanged during composition passthrough.
    assert_eq!(editor.content_string(), "hello");
}

#[wasm_bindgen_test]
fn test_handle_undo_redo() {
    let mut editor = make_editor("hello");
    editor.set_cursor_offset(5);
    let plat = test_platform();

    // Insert text first.
    let insert_ctx = BeforeInputContext {
        input_type: InputType::InsertText,
        data: Some(" world".to_string()),
        target_range: None,
        is_composing: false,
        platform: &plat,
    };
    handle_beforeinput(&mut editor, &insert_ctx, Range::caret(5));
    assert_eq!(editor.content_string(), "hello world");

    // Undo.
    let undo_ctx = BeforeInputContext {
        input_type: InputType::HistoryUndo,
        data: None,
        target_range: None,
        is_composing: false,
        platform: &plat,
    };
    let result = handle_beforeinput(&mut editor, &undo_ctx, Range::caret(11));
    assert!(matches!(result, BeforeInputResult::Handled));
    assert_eq!(editor.content_string(), "hello");

    // Redo.
    let redo_ctx = BeforeInputContext {
        input_type: InputType::HistoryRedo,
        data: None,
        target_range: None,
        is_composing: false,
        platform: &plat,
    };
    let result = handle_beforeinput(&mut editor, &redo_ctx, Range::caret(5));
    assert!(matches!(result, BeforeInputResult::Handled));
    assert_eq!(editor.content_string(), "hello world");
}

#[wasm_bindgen_test]
fn test_handle_insert_paragraph() {
    let mut editor = make_editor("hello");
    editor.set_cursor_offset(5);
    let plat = test_platform();

    let ctx = BeforeInputContext {
        input_type: InputType::InsertParagraph,
        data: None,
        target_range: None,
        is_composing: false,
        platform: &plat,
    };

    let result = handle_beforeinput(&mut editor, &ctx, Range::caret(5));
    assert!(matches!(result, BeforeInputResult::Handled));
    // InsertParagraph inserts double newline.
    assert!(editor.content_string().contains("\n\n"));
}

#[wasm_bindgen_test]
fn test_handle_selection_delete() {
    let mut editor = make_editor("hello world");
    let plat = test_platform();

    let ctx = BeforeInputContext {
        input_type: InputType::DeleteContentBackward,
        data: None,
        target_range: Some(Range::new(5, 11)), // Select " world"
        is_composing: false,
        platform: &plat,
    };

    let result = handle_beforeinput(&mut editor, &ctx, Range::new(5, 11));
    assert!(matches!(result, BeforeInputResult::Handled));
    assert_eq!(editor.content_string(), "hello");
}
