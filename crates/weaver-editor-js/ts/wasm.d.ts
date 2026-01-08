/**
 * Type declarations for the WASM module.
 *
 * These match the exports from wasm-bindgen.
 * The actual module is generated at build time.
 */

declare module "./bundler/weaver_editor.js" {
  export class JsEditor {
    constructor();
    static fromMarkdown(content: string): JsEditor;
    static fromSnapshot(snapshot: unknown): JsEditor;

    // Mounting
    mount(container: HTMLElement, onChange?: () => void): void;
    unmount(): void;
    isMounted(): boolean;
    focus(): void;
    blur(): void;

    // Content
    getMarkdown(): string;
    getSnapshot(): unknown;
    toEntry(): unknown;
    setResolvedContent(content: JsResolvedContent): void;

    // Metadata
    getTitle(): string;
    setTitle(title: string): void;
    getPath(): string;
    setPath(path: string): void;
    getTags(): string[];
    setTags(tags: string[]): void;

    // Actions
    executeAction(action: unknown): void;

    // Images
    addPendingImage(image: unknown, dataUrl: string): void;
    finalizeImage(
      localId: string,
      finalized: unknown,
      blobRkey: string,
      ident: string
    ): void;
    removeImage(localId: string): void;
    getPendingImages(): unknown;
    getStagingUris(): string[];

    // Entry index
    addEntryToIndex(title: string, path: string, canonicalUrl: string): void;
    clearEntryIndex(): void;

    // Cursor
    getCursorOffset(): number;
    setCursorOffset(offset: number): void;
    getLength(): number;

    // Undo/redo
    canUndo(): boolean;
    canRedo(): boolean;

    // Rendering
    getParagraphs(): unknown;
    renderAndUpdateDom(): void;

    // Event handlers
    handleBeforeInput(
      inputType: string,
      data: string | null,
      targetStart: number | null,
      targetEnd: number | null,
      isComposing: boolean
    ): "Handled" | "PassThrough" | "HandledAsync";
    handleKeydown(
      key: string,
      ctrl: boolean,
      alt: boolean,
      shift: boolean,
      meta: boolean
    ): "Handled" | "PassThrough" | "HandledAsync";
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

  export class JsResolvedContent {
    addEmbed(atUri: string, html: string): void;
  }

  export function create_resolved_content(): JsResolvedContent;

  // Default export for WASM init
  export default function init(): Promise<void>;
}
