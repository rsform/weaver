/**
 * Weaver Editor - Embeddable markdown editor.
 *
 * Usage:
 * ```typescript
 * import { createEditor } from '@weaver.sh/editor-core';
 *
 * const editor = await createEditor({
 *   container: document.getElementById('editor')!,
 *   initialMarkdown: '# Hello World',
 *   onChange: () => console.log('changed'),
 * });
 *
 * // Get content
 * const md = editor.getMarkdown();
 * const entry = editor.toEntry();
 *
 * // Cleanup
 * editor.destroy();
 * ```
 */
import type { Editor, EditorConfig, EventResult } from "./types";
export * from "./types";
interface JsResolvedContent {
    addEmbed(atUri: string, html: string): void;
}
interface JsEditor {
    mount(container: HTMLElement, onChange?: () => void): void;
    unmount(): void;
    isMounted(): boolean;
    focus(): void;
    blur(): void;
    getMarkdown(): string;
    getSnapshot(): unknown;
    toEntry(): unknown;
    setResolvedContent(content: JsResolvedContent): void;
    getTitle(): string;
    setTitle(title: string): void;
    getPath(): string;
    setPath(path: string): void;
    getTags(): string[];
    setTags(tags: string[]): void;
    executeAction(action: unknown): void;
    addPendingImage(image: unknown, dataUrl: string): void;
    finalizeImage(localId: string, finalized: unknown, blobRkey: string, ident: string): void;
    removeImage(localId: string): void;
    getPendingImages(): unknown;
    getStagingUris(): string[];
    addEntryToIndex(title: string, path: string, canonicalUrl: string): void;
    clearEntryIndex(): void;
    getCursorOffset(): number;
    setCursorOffset(offset: number): void;
    getLength(): number;
    canUndo(): boolean;
    canRedo(): boolean;
    getParagraphs(): unknown;
    renderAndUpdateDom(): void;
    handleBeforeInput(inputType: string, data: string | null, targetStart: number | null, targetEnd: number | null, isComposing: boolean): EventResult;
    handleKeydown(key: string, ctrl: boolean, alt: boolean, shift: boolean, meta: boolean): EventResult;
    handleKeyup(key: string): void;
    handlePaste(text: string): void;
    handleCut(): string | null;
    handleCopy(): string | null;
    handleBlur(): void;
    handleCompositionStart(data: string | null): void;
    handleCompositionUpdate(data: string | null): void;
    handleCompositionEnd(data: string | null): void;
    handleAndroidEnter(): void;
    syncCursor(): void;
}
interface JsEditorConstructor {
    new (): JsEditor;
    fromMarkdown(content: string): JsEditor;
    fromSnapshot(snapshot: unknown): JsEditor;
}
interface WasmModule {
    JsEditor: JsEditorConstructor;
    create_resolved_content: () => JsResolvedContent;
}
/**
 * Initialize the WASM module.
 *
 * Called automatically by createEditor, but can be called early
 * to preload the module.
 */
export declare function initWasm(): Promise<WasmModule>;
/**
 * Create a new editor instance.
 */
export declare function createEditor(config: EditorConfig): Promise<Editor>;
export { createCollabEditor, initCollabWasm } from "./collab";
//# sourceMappingURL=index.d.ts.map