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
// Re-export types
export * from "./types";
let wasmModule = null;
/**
 * Initialize the WASM module.
 *
 * Called automatically by createEditor, but can be called early
 * to preload the module.
 */
export async function initWasm() {
    if (wasmModule)
        return wasmModule;
    // Dynamic import of the generated WASM bindings
    // The bundler/ dir is symlinked from pkg/core/bundler during build
    const mod = await import("./bundler/weaver_editor.js");
    wasmModule = mod;
    return wasmModule;
}
/**
 * Create a new editor instance.
 */
export async function createEditor(config) {
    const wasm = await initWasm();
    // Create the inner WASM editor
    let inner;
    if (config.initialSnapshot) {
        inner = wasm.JsEditor.fromSnapshot(config.initialSnapshot);
    }
    else if (config.initialMarkdown) {
        inner = wasm.JsEditor.fromMarkdown(config.initialMarkdown);
    }
    else {
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
class EditorImpl {
    constructor(inner, config) {
        this.container = null;
        this.editorElement = null;
        this.destroyed = false;
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
    mountToContainer(container) {
        this.container = container;
        // Mount creates the contenteditable element
        this.inner.mount(container, this.config.onChange);
        // Find the created editor element
        const editorEl = container.querySelector(".weaver-editor-content");
        if (!editorEl) {
            throw new Error("Failed to find editor element after mount");
        }
        this.editorElement = editorEl;
        // Set up event listeners
        this.attachEventListeners();
    }
    attachEventListeners() {
        const el = this.editorElement;
        if (!el)
            return;
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
    detachEventListeners() {
        const el = this.editorElement;
        if (!el)
            return;
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
    onBeforeInput(e) {
        const inputType = e.inputType;
        const data = e.data ?? null;
        // Get target ranges
        let targetStart = null;
        let targetEnd = null;
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
    onKeydown(e) {
        const result = this.inner.handleKeydown(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
        if (result === "Handled") {
            e.preventDefault();
        }
    }
    onKeyup(e) {
        this.inner.handleKeyup(e.key);
    }
    onPaste(e) {
        e.preventDefault();
        const text = e.clipboardData?.getData("text/plain") ?? "";
        this.inner.handlePaste(text);
    }
    onCut(e) {
        e.preventDefault();
        const text = this.inner.handleCut();
        if (text && e.clipboardData) {
            e.clipboardData.setData("text/plain", text);
        }
    }
    onCopy(e) {
        e.preventDefault();
        const text = this.inner.handleCopy();
        if (text && e.clipboardData) {
            e.clipboardData.setData("text/plain", text);
        }
    }
    onBlur() {
        this.inner.handleBlur();
    }
    onCompositionStart(e) {
        this.inner.handleCompositionStart(e.data ?? null);
    }
    onCompositionUpdate(e) {
        this.inner.handleCompositionUpdate(e.data ?? null);
    }
    onCompositionEnd(e) {
        this.inner.handleCompositionEnd(e.data ?? null);
    }
    onMouseUp() {
        // Sync cursor after mouse selection
        this.inner.syncCursor();
    }
    onTouchEnd() {
        // Sync cursor after touch selection
        this.inner.syncCursor();
    }
    /** Convert DOM node/offset to character offset. */
    domOffsetToChar(node, offset) {
        // Walk the DOM to calculate character offset
        // This needs to match the WASM side's paragraph structure
        const editor = this.editorElement;
        if (!editor)
            return null;
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
    getMarkdown() {
        this.checkDestroyed();
        return this.inner.getMarkdown();
    }
    getSnapshot() {
        this.checkDestroyed();
        return this.inner.getSnapshot();
    }
    toEntry() {
        this.checkDestroyed();
        return this.inner.toEntry();
    }
    getTitle() {
        this.checkDestroyed();
        return this.inner.getTitle();
    }
    setTitle(title) {
        this.checkDestroyed();
        this.inner.setTitle(title);
    }
    getPath() {
        this.checkDestroyed();
        return this.inner.getPath();
    }
    setPath(path) {
        this.checkDestroyed();
        this.inner.setPath(path);
    }
    getTags() {
        this.checkDestroyed();
        return this.inner.getTags();
    }
    setTags(tags) {
        this.checkDestroyed();
        this.inner.setTags(tags);
    }
    executeAction(action) {
        this.checkDestroyed();
        this.inner.executeAction(action);
    }
    addPendingImage(image, dataUrl) {
        this.checkDestroyed();
        this.inner.addPendingImage(image, dataUrl);
        this.config.onImageAdd?.(image);
    }
    finalizeImage(localId, finalized, blobRkey, identifier) {
        this.checkDestroyed();
        this.inner.finalizeImage(localId, finalized, blobRkey, identifier);
    }
    removeImage(localId) {
        this.checkDestroyed();
        this.inner.removeImage(localId);
    }
    getPendingImages() {
        this.checkDestroyed();
        return this.inner.getPendingImages();
    }
    getStagingUris() {
        this.checkDestroyed();
        return this.inner.getStagingUris();
    }
    addEntryToIndex(title, path, canonicalUrl) {
        this.checkDestroyed();
        this.inner.addEntryToIndex(title, path, canonicalUrl);
    }
    clearEntryIndex() {
        this.checkDestroyed();
        this.inner.clearEntryIndex();
    }
    getCursorOffset() {
        this.checkDestroyed();
        return this.inner.getCursorOffset();
    }
    setCursorOffset(offset) {
        this.checkDestroyed();
        this.inner.setCursorOffset(offset);
    }
    getLength() {
        this.checkDestroyed();
        return this.inner.getLength();
    }
    canUndo() {
        this.checkDestroyed();
        return this.inner.canUndo();
    }
    canRedo() {
        this.checkDestroyed();
        return this.inner.canRedo();
    }
    focus() {
        this.checkDestroyed();
        this.inner.focus();
    }
    blur() {
        this.checkDestroyed();
        this.inner.blur();
    }
    getParagraphs() {
        this.checkDestroyed();
        return this.inner.getParagraphs();
    }
    renderAndUpdateDom() {
        this.checkDestroyed();
        this.inner.renderAndUpdateDom();
    }
    destroy() {
        if (this.destroyed)
            return;
        this.destroyed = true;
        this.detachEventListeners();
        this.inner.unmount();
        this.container = null;
        this.editorElement = null;
    }
    checkDestroyed() {
        if (this.destroyed) {
            throw new Error("Editor has been destroyed");
        }
    }
}
// Re-export collab module
export { createCollabEditor, initCollabWasm } from "./collab";
