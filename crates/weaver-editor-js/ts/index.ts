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

import type {
  Editor,
  EditorAction,
  EditorConfig,
  EntryJson,
  EventResult,
  FinalizedImage,
  ParagraphRender,
  PendingImage,
} from "./types";

// Re-export types
export * from "./types";

// Internal types for WASM module (matches wasm-bindgen output)
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
  handleBeforeInput(
    inputType: string,
    data: string | null,
    targetStart: number | null,
    targetEnd: number | null,
    isComposing: boolean,
  ): EventResult;
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

let wasmModule: WasmModule | null = null;

/**
 * Initialize the WASM module.
 *
 * Called automatically by createEditor, but can be called early
 * to preload the module.
 */
export async function initWasm(): Promise<WasmModule> {
  if (wasmModule) return wasmModule;

  // Dynamic import of the generated WASM bindings
  // The bundler/ dir is symlinked from pkg/core/bundler during build
  const mod = await import("./bundler/weaver_editor.js");
  wasmModule = mod as unknown as WasmModule;
  return wasmModule;
}

/**
 * Create a new editor instance.
 */
export async function createEditor(config: EditorConfig): Promise<Editor> {
  const wasm = await initWasm();

  // Create the inner WASM editor
  let inner: JsEditor;
  if (config.initialSnapshot) {
    inner = wasm.JsEditor.fromSnapshot(config.initialSnapshot);
  } else if (config.initialMarkdown) {
    inner = wasm.JsEditor.fromMarkdown(config.initialMarkdown);
  } else {
    inner = new wasm.JsEditor();
  }

  // Set up resolved content if provided
  if (config.resolvedContent) {
    const resolved = wasm.create_resolved_content();
    for (const [uri, html] of config.resolvedContent.embeds) {
      resolved.addEmbed(uri, html);
    }
    inner.setResolvedContent(resolved);
  }

  // Create wrapper
  const editor = new EditorImpl(inner, config);

  // Mount to container
  editor.mountToContainer(config.container);

  return editor;
}

/**
 * Internal editor implementation.
 */
class EditorImpl implements Editor {
  private inner: JsEditor;
  private config: EditorConfig;
  private container: HTMLElement | null = null;
  private editorElement: HTMLElement | null = null;
  private destroyed = false;

  // Event handler refs for cleanup
  private boundHandlers: {
    beforeinput: (e: InputEvent) => void;
    keydown: (e: KeyboardEvent) => void;
    keyup: (e: KeyboardEvent) => void;
    paste: (e: ClipboardEvent) => void;
    cut: (e: ClipboardEvent) => void;
    copy: (e: ClipboardEvent) => void;
    blur: () => void;
    compositionstart: (e: CompositionEvent) => void;
    compositionupdate: (e: CompositionEvent) => void;
    compositionend: (e: CompositionEvent) => void;
    mouseup: () => void;
    touchend: () => void;
  };

  constructor(inner: JsEditor, config: EditorConfig) {
    this.inner = inner;
    this.config = config;

    // Bind event handlers
    this.boundHandlers = {
      beforeinput: this.onBeforeInput.bind(this),
      keydown: this.onKeydown.bind(this),
      keyup: this.onKeyup.bind(this),
      paste: this.onPaste.bind(this),
      cut: this.onCut.bind(this),
      copy: this.onCopy.bind(this),
      blur: this.onBlur.bind(this),
      compositionstart: this.onCompositionStart.bind(this),
      compositionupdate: this.onCompositionUpdate.bind(this),
      compositionend: this.onCompositionEnd.bind(this),
      mouseup: this.onMouseUp.bind(this),
      touchend: this.onTouchEnd.bind(this),
    };
  }

  /** Mount to container and set up event listeners. */
  mountToContainer(container: HTMLElement): void {
    this.container = container;

    // Mount creates the contenteditable element
    this.inner.mount(container, this.config.onChange);

    // Find the created editor element
    const editorEl = container.querySelector(".weaver-editor-content") as HTMLElement;
    if (!editorEl) {
      throw new Error("Failed to find editor element after mount");
    }
    this.editorElement = editorEl;

    // Set up event listeners
    this.attachEventListeners();
  }

  private attachEventListeners(): void {
    const el = this.editorElement;
    if (!el) return;

    el.addEventListener("beforeinput", this.boundHandlers.beforeinput);
    el.addEventListener("keydown", this.boundHandlers.keydown);
    el.addEventListener("keyup", this.boundHandlers.keyup);
    el.addEventListener("paste", this.boundHandlers.paste);
    el.addEventListener("cut", this.boundHandlers.cut);
    el.addEventListener("copy", this.boundHandlers.copy);
    el.addEventListener("blur", this.boundHandlers.blur);
    el.addEventListener("compositionstart", this.boundHandlers.compositionstart);
    el.addEventListener("compositionupdate", this.boundHandlers.compositionupdate);
    el.addEventListener("compositionend", this.boundHandlers.compositionend);
    el.addEventListener("mouseup", this.boundHandlers.mouseup);
    el.addEventListener("touchend", this.boundHandlers.touchend);
  }

  private detachEventListeners(): void {
    const el = this.editorElement;
    if (!el) return;

    el.removeEventListener("beforeinput", this.boundHandlers.beforeinput);
    el.removeEventListener("keydown", this.boundHandlers.keydown);
    el.removeEventListener("keyup", this.boundHandlers.keyup);
    el.removeEventListener("paste", this.boundHandlers.paste);
    el.removeEventListener("cut", this.boundHandlers.cut);
    el.removeEventListener("copy", this.boundHandlers.copy);
    el.removeEventListener("blur", this.boundHandlers.blur);
    el.removeEventListener("compositionstart", this.boundHandlers.compositionstart);
    el.removeEventListener("compositionupdate", this.boundHandlers.compositionupdate);
    el.removeEventListener("compositionend", this.boundHandlers.compositionend);
    el.removeEventListener("mouseup", this.boundHandlers.mouseup);
    el.removeEventListener("touchend", this.boundHandlers.touchend);
  }

  // === Event handlers ===

  private onBeforeInput(e: InputEvent): void {
    const inputType = e.inputType;
    const data = e.data ?? null;

    // Get target ranges
    let targetStart: number | null = null;
    let targetEnd: number | null = null;
    const ranges = e.getTargetRanges?.();
    if (ranges && ranges.length > 0) {
      // Convert DOM range to character offsets
      // This is simplified - real impl needs offset calculation
      const range = ranges[0];
      targetStart = this.domOffsetToChar(range.startContainer, range.startOffset);
      targetEnd = this.domOffsetToChar(range.endContainer, range.endOffset);
    }

    const isComposing = e.isComposing;
    const result = this.inner.handleBeforeInput(inputType, data, targetStart, targetEnd, isComposing);

    if (result === "Handled" || result === "HandledAsync") {
      e.preventDefault();
    }
  }

  private onKeydown(e: KeyboardEvent): void {
    const result = this.inner.handleKeydown(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);

    if (result === "Handled") {
      e.preventDefault();
    }
  }

  private onKeyup(e: KeyboardEvent): void {
    this.inner.handleKeyup(e.key);
  }

  private onPaste(e: ClipboardEvent): void {
    e.preventDefault();
    const text = e.clipboardData?.getData("text/plain") ?? "";
    this.inner.handlePaste(text);
  }

  private onCut(e: ClipboardEvent): void {
    e.preventDefault();
    const text = this.inner.handleCut();
    if (text && e.clipboardData) {
      e.clipboardData.setData("text/plain", text);
    }
  }

  private onCopy(e: ClipboardEvent): void {
    e.preventDefault();
    const text = this.inner.handleCopy();
    if (text && e.clipboardData) {
      e.clipboardData.setData("text/plain", text);
    }
  }

  private onBlur(): void {
    this.inner.handleBlur();
  }

  private onCompositionStart(e: CompositionEvent): void {
    this.inner.handleCompositionStart(e.data ?? null);
  }

  private onCompositionUpdate(e: CompositionEvent): void {
    this.inner.handleCompositionUpdate(e.data ?? null);
  }

  private onCompositionEnd(e: CompositionEvent): void {
    this.inner.handleCompositionEnd(e.data ?? null);
  }

  private onMouseUp(): void {
    // Sync cursor after mouse selection
    this.inner.syncCursor();
  }

  private onTouchEnd(): void {
    // Sync cursor after touch selection
    this.inner.syncCursor();
  }

  /** Convert DOM node/offset to character offset. */
  private domOffsetToChar(node: Node, offset: number): number | null {
    // Walk the DOM to calculate character offset
    // This needs to match the WASM side's paragraph structure
    const editor = this.editorElement;
    if (!editor) return null;

    let charOffset = 0;
    const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT);

    let currentNode = walker.nextNode();
    while (currentNode) {
      if (currentNode === node) {
        return charOffset + offset;
      }
      charOffset += currentNode.textContent?.length ?? 0;
      currentNode = walker.nextNode();
    }

    // Node not found, might be element node
    if (node.nodeType === Node.ELEMENT_NODE) {
      // Count text length of child nodes up to offset
      for (let i = 0; i < offset && i < node.childNodes.length; i++) {
        charOffset += node.childNodes[i].textContent?.length ?? 0;
      }
      return charOffset;
    }

    return null;
  }

  // === Public API ===

  getMarkdown(): string {
    this.checkDestroyed();
    return this.inner.getMarkdown();
  }

  getSnapshot(): EntryJson {
    this.checkDestroyed();
    return this.inner.getSnapshot() as EntryJson;
  }

  toEntry(): EntryJson {
    this.checkDestroyed();
    return this.inner.toEntry() as EntryJson;
  }

  getTitle(): string {
    this.checkDestroyed();
    return this.inner.getTitle();
  }

  setTitle(title: string): void {
    this.checkDestroyed();
    this.inner.setTitle(title);
  }

  getPath(): string {
    this.checkDestroyed();
    return this.inner.getPath();
  }

  setPath(path: string): void {
    this.checkDestroyed();
    this.inner.setPath(path);
  }

  getTags(): string[] {
    this.checkDestroyed();
    return this.inner.getTags();
  }

  setTags(tags: string[]): void {
    this.checkDestroyed();
    this.inner.setTags(tags);
  }

  executeAction(action: EditorAction): void {
    this.checkDestroyed();
    this.inner.executeAction(action);
  }

  addPendingImage(image: PendingImage, dataUrl: string): void {
    this.checkDestroyed();
    this.inner.addPendingImage(image, dataUrl);
    this.config.onImageAdd?.(image);
  }

  finalizeImage(localId: string, finalized: FinalizedImage, blobRkey: string, identifier: string): void {
    this.checkDestroyed();
    this.inner.finalizeImage(localId, finalized, blobRkey, identifier);
  }

  removeImage(localId: string): void {
    this.checkDestroyed();
    this.inner.removeImage(localId);
  }

  getPendingImages(): PendingImage[] {
    this.checkDestroyed();
    return this.inner.getPendingImages() as PendingImage[];
  }

  getStagingUris(): string[] {
    this.checkDestroyed();
    return this.inner.getStagingUris();
  }

  addEntryToIndex(title: string, path: string, canonicalUrl: string): void {
    this.checkDestroyed();
    this.inner.addEntryToIndex(title, path, canonicalUrl);
  }

  clearEntryIndex(): void {
    this.checkDestroyed();
    this.inner.clearEntryIndex();
  }

  getCursorOffset(): number {
    this.checkDestroyed();
    return this.inner.getCursorOffset();
  }

  setCursorOffset(offset: number): void {
    this.checkDestroyed();
    this.inner.setCursorOffset(offset);
  }

  getLength(): number {
    this.checkDestroyed();
    return this.inner.getLength();
  }

  canUndo(): boolean {
    this.checkDestroyed();
    return this.inner.canUndo();
  }

  canRedo(): boolean {
    this.checkDestroyed();
    return this.inner.canRedo();
  }

  focus(): void {
    this.checkDestroyed();
    this.inner.focus();
  }

  blur(): void {
    this.checkDestroyed();
    this.inner.blur();
  }

  getParagraphs(): ParagraphRender[] {
    this.checkDestroyed();
    return this.inner.getParagraphs() as ParagraphRender[];
  }

  renderAndUpdateDom(): void {
    this.checkDestroyed();
    this.inner.renderAndUpdateDom();
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;

    this.detachEventListeners();
    this.inner.unmount();
    this.container = null;
    this.editorElement = null;
  }

  private checkDestroyed(): void {
    if (this.destroyed) {
      throw new Error("Editor has been destroyed");
    }
  }
}
