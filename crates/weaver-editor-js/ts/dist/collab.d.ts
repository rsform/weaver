/**
 * Collaborative editor with Loro CRDT and iroh P2P.
 *
 * Usage:
 * ```typescript
 * import { createCollabEditor } from '@weaver.sh/editor-collab';
 *
 * const editor = await createCollabEditor({
 *   container: document.getElementById('editor')!,
 *   resourceUri: 'at://did:plc:abc/sh.weaver.notebook.entry/xyz',
 *   onChange: () => console.log('changed'),
 *   onSessionNeeded: async (session) => {
 *     // Create session record on PDS, return URI
 *     return 'at://did:plc:abc/sh.weaver.edit.session/123';
 *   },
 *   onPeersNeeded: async (resourceUri) => {
 *     // Query index/backlinks for peer session records
 *     return [{ nodeId: 'peer-node-id' }];
 *   },
 * });
 *
 * // Get Loro snapshot for saving
 * const snapshot = editor.exportSnapshot();
 *
 * // Cleanup
 * await editor.stopCollab();
 * editor.destroy();
 * ```
 */
import type { CollabEditor, CollabEditorConfig, CursorRect, EventResult, PeerInfo, PresenceSnapshot, Selection, SelectionRect, SessionInfo } from "./types";
interface JsCollabEditor {
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
    getSelection(): Selection | null;
    setCursorOffset(offset: number): void;
    getLength(): number;
    canUndo(): boolean;
    canRedo(): boolean;
    getParagraphs(): unknown;
    renderAndUpdateDom(): void;
    getCursorRectRelative(position: number): CursorRect | null;
    getSelectionRectsRelative(start: number, end: number): SelectionRect[];
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
    exportSnapshot(): Uint8Array;
    exportUpdatesSince(version: Uint8Array): Uint8Array | null;
    importUpdates(data: Uint8Array): void;
    getVersion(): Uint8Array;
    getCollabTopic(): Uint8Array | null;
    getResourceUri(): string;
    setOnSessionNeeded(callback: (info: SessionInfo) => Promise<string>): void;
    setOnSessionRefresh(callback: (uri: string) => Promise<void>): void;
    setOnSessionEnd(callback: (uri: string) => Promise<void>): void;
    setOnPeersNeeded(callback: (uri: string) => Promise<PeerInfo[]>): void;
    setOnPresenceChanged(callback: (presence: PresenceSnapshot) => void): void;
    setOnRemoteUpdate(callback: () => void): void;
}
interface JsCollabEditorConstructor {
    new (resourceUri: string): JsCollabEditor;
    fromMarkdown(resourceUri: string, content: string): JsCollabEditor;
    fromSnapshot(resourceUri: string, snapshot: Uint8Array): JsCollabEditor;
}
interface JsResolvedContent {
    addEmbed(atUri: string, html: string): void;
}
interface CollabWasmModule {
    JsCollabEditor: JsCollabEditorConstructor;
    create_resolved_content: () => JsResolvedContent;
}
/**
 * Initialize the collab WASM module.
 */
export declare function initCollabWasm(): Promise<CollabWasmModule>;
/**
 * Create a new collaborative editor instance.
 *
 * @param config Editor configuration
 * @param workerUrl URL to the editor_worker.js file (default: "/worker/editor_worker.js")
 */
export declare function createCollabEditor(config: CollabEditorConfig, workerUrl?: string): Promise<CollabEditor>;
export {};
//# sourceMappingURL=collab.d.ts.map