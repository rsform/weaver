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

import type {
  CollabEditor,
  CollabEditorConfig,
  CursorRect,
  EditorAction,
  EntryJson,
  EventResult,
  FinalizedImage,
  ParagraphRender,
  PeerInfo,
  PendingImage,
  PresenceSnapshot,
  Selection,
  SelectionRect,
  SessionInfo,
  UserInfo,
} from "./types";

// ============================================================
// Color utilities
// ============================================================

/** Convert RGBA u32 (0xRRGGBBAA) to CSS rgba() string. */
function rgbaToCss(color: number): string {
  const r = (color >>> 24) & 0xff;
  const g = (color >>> 16) & 0xff;
  const b = (color >>> 8) & 0xff;
  const a = (color & 0xff) / 255;
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

/** Convert RGBA u32 to CSS rgba() string with custom alpha. */
function rgbaToCssAlpha(color: number, alpha: number): string {
  const r = (color >>> 24) & 0xff;
  const g = (color >>> 16) & 0xff;
  const b = (color >>> 8) & 0xff;
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

// ============================================================
// Worker message types (must match Rust WorkerInput/WorkerOutput)
// ============================================================

type WorkerInput =
  | { type: "Init"; snapshot: number[]; draft_key: string }
  | { type: "ApplyUpdates"; updates: number[] }
  | {
      type: "ExportSnapshot";
      cursor_offset: number;
      editing_uri: string | null;
      editing_cid: string | null;
      notebook_uri: string | null;
    }
  | { type: "StartCollab"; topic: number[]; bootstrap_peers: string[] }
  | { type: "BroadcastUpdate"; data: number[] }
  | { type: "AddPeers"; peers: string[] }
  | { type: "BroadcastJoin"; did: string; display_name: string }
  | { type: "BroadcastCursor"; position: number; selection: [number, number] | null }
  | { type: "StopCollab" };

type WorkerOutput =
  | { type: "Ready" }
  | {
      type: "Snapshot";
      draft_key: string;
      b64_snapshot: string;
      content: string;
      title: string;
      cursor_offset: number;
      editing_uri: string | null;
      editing_cid: string | null;
      notebook_uri: string | null;
      export_ms: number;
      encode_ms: number;
    }
  | { type: "Error"; message: string }
  | { type: "CollabReady"; node_id: string; relay_url: string | null }
  | { type: "CollabJoined" }
  | { type: "RemoteUpdates"; data: number[] }
  | { type: "PresenceUpdate"; collaborators: PresenceSnapshot["collaborators"]; peer_count: number }
  | { type: "CollabStopped" }
  | { type: "PeerConnected" };

// ============================================================
// Worker Bridge
// ============================================================

/**
 * Bridge to communicate with the EditorReactor web worker.
 *
 * The worker handles:
 * - CPU-intensive Loro operations off main thread
 * - iroh P2P networking for real-time collaboration
 */
class WorkerBridge {
  private worker: Worker | null = null;
  private messageHandlers: ((msg: WorkerOutput) => void)[] = [];
  private pendingReady: ((value: void) => void) | null = null;

  /**
   * Spawn the worker. Must be called before any other methods.
   *
   * @param workerUrl URL to the worker JS file (editor_worker.js)
   */
  async spawn(workerUrl: string): Promise<void> {
    if (this.worker) {
      throw new Error("Worker already spawned");
    }

    return new Promise((resolve, reject) => {
      try {
        this.worker = new Worker(workerUrl);

        this.worker.onmessage = (e: MessageEvent) => {
          const msg = e.data as WorkerOutput;
          this.handleMessage(msg);
        };

        this.worker.onerror = (e: ErrorEvent) => {
          console.error("Worker error:", e);
          reject(new Error(`Worker error: ${e.message}`));
        };

        // Wait for Ready message
        this.pendingReady = resolve;
      } catch (err) {
        reject(err);
      }
    });
  }

  /**
   * Send a message to the worker.
   */
  send(msg: WorkerInput): void {
    if (!this.worker) {
      throw new Error("Worker not spawned");
    }
    this.worker.postMessage(msg);
  }

  /**
   * Register a handler for worker messages.
   */
  onMessage(handler: (msg: WorkerOutput) => void): () => void {
    this.messageHandlers.push(handler);
    return () => {
      const idx = this.messageHandlers.indexOf(handler);
      if (idx >= 0) {
        this.messageHandlers.splice(idx, 1);
      }
    };
  }

  /**
   * Terminate the worker.
   */
  terminate(): void {
    if (this.worker) {
      this.worker.terminate();
      this.worker = null;
    }
    this.messageHandlers = [];
  }

  private handleMessage(msg: WorkerOutput): void {
    // Handle Ready specially to resolve spawn promise
    if (msg.type === "Ready" && this.pendingReady) {
      this.pendingReady();
      this.pendingReady = null;
    }

    // Dispatch to all handlers
    for (const handler of this.messageHandlers) {
      try {
        handler(msg);
      } catch (err) {
        console.error("Error in worker message handler:", err);
      }
    }
  }
}

// Internal types for WASM module
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

  // Remote cursor positioning
  getCursorRectRelative(position: number): CursorRect | null;
  getSelectionRectsRelative(start: number, end: number): SelectionRect[];
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

  // Loro sync methods
  exportSnapshot(): Uint8Array;
  exportUpdatesSince(version: Uint8Array): Uint8Array | null;
  importUpdates(data: Uint8Array): void;
  getVersion(): Uint8Array;
  getCollabTopic(): Uint8Array | null;
  getResourceUri(): string;

  // Callbacks
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

let wasmModule: CollabWasmModule | null = null;

/**
 * Initialize the collab WASM module.
 */
export async function initCollabWasm(): Promise<CollabWasmModule> {
  if (wasmModule) return wasmModule;

  // The collab module is built separately with the collab feature
  const mod = await import("./bundler/weaver_editor.js");
  wasmModule = mod as unknown as CollabWasmModule;
  return wasmModule;
}

/**
 * Create a new collaborative editor instance.
 *
 * @param config Editor configuration
 * @param workerUrl URL to the editor_worker.js file (default: "/worker/editor_worker.js")
 */
export async function createCollabEditor(
  config: CollabEditorConfig,
  workerUrl = "/worker/editor_worker.js",
): Promise<CollabEditor> {
  const wasm = await initCollabWasm();

  // Create the inner WASM editor
  let inner: JsCollabEditor;
  if (config.initialLoroSnapshot) {
    inner = wasm.JsCollabEditor.fromSnapshot(config.resourceUri, config.initialLoroSnapshot);
  } else if (config.initialMarkdown) {
    inner = wasm.JsCollabEditor.fromMarkdown(config.resourceUri, config.initialMarkdown);
  } else {
    inner = new wasm.JsCollabEditor(config.resourceUri);
  }

  // Set up resolved content if provided
  if (config.resolvedContent) {
    const resolved = wasm.create_resolved_content();
    for (const [uri, html] of config.resolvedContent.embeds) {
      resolved.addEmbed(uri, html);
    }
    inner.setResolvedContent(resolved);
  }

  // Create wrapper with worker URL
  const editor = new CollabEditorImpl(inner, config, workerUrl);

  // Mount to container
  editor.mountToContainer(config.container);

  return editor;
}

/**
 * Internal collab editor implementation.
 */
class CollabEditorImpl implements CollabEditor {
  private inner: JsCollabEditor;
  private config: CollabEditorConfig;
  private container: HTMLElement | null = null;
  private editorElement: HTMLElement | null = null;
  private destroyed = false;

  // Worker bridge for P2P collab
  private workerBridge: WorkerBridge | null = null;
  private workerUrl: string;
  private sessionUri: string | null = null;
  private collabStarted = false;
  private unsubscribeWorker: (() => void) | null = null;
  private lastSyncedVersion: Uint8Array | null = null;
  private lastBroadcastCursor: number = -1;

  // Remote cursor overlay
  private currentPresence: PresenceSnapshot | null = null;
  private cursorOverlay: HTMLElement | null = null;

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

  constructor(inner: JsCollabEditor, config: CollabEditorConfig, workerUrl: string) {
    this.inner = inner;
    this.config = config;
    this.workerUrl = workerUrl;

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

    // Wrap onChange to also sync updates to worker
    const wrappedOnChange = () => {
      this.syncToWorker();
      this.config.onChange?.();
      // Re-render remote cursors after content changes (positions may shift)
      this.renderRemoteCursors();
    };

    this.inner.mount(container, wrappedOnChange);

    const editorEl = container.querySelector(".weaver-editor-content") as HTMLElement;
    if (!editorEl) {
      throw new Error("Failed to find editor element after mount");
    }
    this.editorElement = editorEl;
    this.attachEventListeners();

    // Create remote cursors overlay
    this.cursorOverlay = document.createElement("div");
    this.cursorOverlay.className = "remote-cursors-overlay";
    container.appendChild(this.cursorOverlay);

    // Initialize synced version
    this.lastSyncedVersion = this.inner.getVersion();
  }

  /**
   * Sync local changes to the worker for broadcast.
   */
  private syncToWorker(): void {
    if (!this.workerBridge || !this.collabStarted || !this.lastSyncedVersion) {
      return;
    }

    // Export updates since last sync
    const updates = this.inner.exportUpdatesSince(this.lastSyncedVersion);
    if (updates) {
      // Send to worker for broadcast
      this.workerBridge.send({
        type: "BroadcastUpdate",
        data: Array.from(updates),
      });

      // Also send to worker to keep shadow doc in sync
      this.workerBridge.send({
        type: "ApplyUpdates",
        updates: Array.from(updates),
      });

      // Update synced version
      this.lastSyncedVersion = this.inner.getVersion();
    }

    // Also sync cursor
    this.broadcastCursor();
  }

  /**
   * Render remote collaborator cursors.
   */
  private renderRemoteCursors(): void {
    if (!this.cursorOverlay || !this.currentPresence) {
      return;
    }

    // Clear existing cursors
    this.cursorOverlay.innerHTML = "";

    for (const collab of this.currentPresence.collaborators) {
      if (collab.cursorPosition === undefined) {
        continue;
      }

      const rect = this.inner.getCursorRectRelative(collab.cursorPosition);
      if (!rect) {
        continue;
      }

      // Convert color to CSS
      const colorCss = rgbaToCss(collab.color);
      const selectionColorCss = rgbaToCssAlpha(collab.color, 0.25);

      // Render selection highlights first (behind cursor)
      if (collab.selection) {
        const [start, end] = collab.selection;
        const [selStart, selEnd] = start <= end ? [start, end] : [end, start];
        const selRects = this.inner.getSelectionRectsRelative(selStart, selEnd);

        for (const selRect of selRects) {
          const selDiv = document.createElement("div");
          selDiv.className = "remote-selection";
          selDiv.style.cssText = `
            left: ${selRect.x}px;
            top: ${selRect.y}px;
            width: ${selRect.width}px;
            height: ${selRect.height}px;
            background-color: ${selectionColorCss};
          `;
          this.cursorOverlay.appendChild(selDiv);
        }
      }

      // Create cursor element
      const cursorDiv = document.createElement("div");
      cursorDiv.className = "remote-cursor";
      cursorDiv.style.cssText = `
        left: ${rect.x}px;
        top: ${rect.y}px;
        --cursor-height: ${rect.height}px;
        --cursor-color: ${colorCss};
      `;

      // Caret line
      const caretDiv = document.createElement("div");
      caretDiv.className = "remote-cursor-caret";
      cursorDiv.appendChild(caretDiv);

      // Name label
      const labelDiv = document.createElement("div");
      labelDiv.className = "remote-cursor-label";
      labelDiv.textContent = collab.displayName;
      cursorDiv.appendChild(labelDiv);

      this.cursorOverlay.appendChild(cursorDiv);
    }
  }

  /**
   * Broadcast cursor position to peers.
   */
  private broadcastCursor(): void {
    if (!this.workerBridge || !this.collabStarted) {
      return;
    }

    const cursor = this.inner.getCursorOffset();
    const sel = this.inner.getSelection();

    // Only broadcast if cursor changed
    if (cursor === this.lastBroadcastCursor && !sel) {
      return;
    }

    this.lastBroadcastCursor = cursor;

    this.workerBridge.send({
      type: "BroadcastCursor",
      position: cursor,
      selection: sel ? [sel.anchor, sel.head] : null,
    });
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

  // === Event handlers (same as EditorImpl) ===

  private onBeforeInput(e: InputEvent): void {
    const inputType = e.inputType;
    const data = e.data ?? null;

    let targetStart: number | null = null;
    let targetEnd: number | null = null;
    const ranges = e.getTargetRanges?.();
    if (ranges && ranges.length > 0) {
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
    this.inner.syncCursor();
    this.broadcastCursor();
  }

  private onTouchEnd(): void {
    this.inner.syncCursor();
    this.broadcastCursor();
  }

  private domOffsetToChar(node: Node, offset: number): number | null {
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

    if (node.nodeType === Node.ELEMENT_NODE) {
      for (let i = 0; i < offset && i < node.childNodes.length; i++) {
        charOffset += node.childNodes[i].textContent?.length ?? 0;
      }
      return charOffset;
    }

    return null;
  }

  // === Loro sync methods ===

  exportSnapshot(): Uint8Array {
    this.checkDestroyed();
    return this.inner.exportSnapshot();
  }

  exportUpdatesSince(version: Uint8Array): Uint8Array | null {
    this.checkDestroyed();
    return this.inner.exportUpdatesSince(version);
  }

  importUpdates(data: Uint8Array): void {
    this.checkDestroyed();
    this.inner.importUpdates(data);
  }

  getVersion(): Uint8Array {
    this.checkDestroyed();
    return this.inner.getVersion();
  }

  getCollabTopic(): Uint8Array | null {
    this.checkDestroyed();
    return this.inner.getCollabTopic();
  }

  getResourceUri(): string {
    this.checkDestroyed();
    return this.inner.getResourceUri();
  }

  // === Collab lifecycle ===

  async startCollab(bootstrapPeers?: string[]): Promise<void> {
    this.checkDestroyed();

    if (this.collabStarted) {
      console.warn("Collab already started");
      return;
    }

    // Spawn worker
    this.workerBridge = new WorkerBridge();
    await this.workerBridge.spawn(this.workerUrl);

    // Set up message handler
    this.unsubscribeWorker = this.workerBridge.onMessage((msg) => {
      this.handleWorkerMessage(msg);
    });

    // Initialize worker with current Loro snapshot
    const snapshot = this.inner.exportSnapshot();
    this.workerBridge.send({
      type: "Init",
      snapshot: Array.from(snapshot),
      draft_key: this.config.resourceUri,
    });

    // Start collab session
    const topic = this.inner.getCollabTopic();
    if (!topic) {
      throw new Error("No collab topic available");
    }

    this.workerBridge.send({
      type: "StartCollab",
      topic: Array.from(topic),
      bootstrap_peers: bootstrapPeers ?? [],
    });

    this.collabStarted = true;
  }

  async stopCollab(): Promise<void> {
    this.checkDestroyed();

    if (!this.collabStarted || !this.workerBridge) {
      return;
    }

    // Send stop to worker
    this.workerBridge.send({ type: "StopCollab" });

    // Delete session record via callback
    if (this.sessionUri && this.config.onSessionEnd) {
      try {
        await this.config.onSessionEnd(this.sessionUri);
      } catch (err) {
        console.error("Failed to delete session record:", err);
      }
    }

    // Clean up
    if (this.unsubscribeWorker) {
      this.unsubscribeWorker();
      this.unsubscribeWorker = null;
    }
    this.workerBridge.terminate();
    this.workerBridge = null;
    this.sessionUri = null;
    this.collabStarted = false;
  }

  addPeers(nodeIds: string[]): void {
    this.checkDestroyed();

    if (!this.workerBridge || !this.collabStarted) {
      console.warn("Cannot add peers - collab not started");
      return;
    }

    this.workerBridge.send({
      type: "AddPeers",
      peers: nodeIds,
    });
  }

  /**
   * Handle messages from the worker.
   */
  private async handleWorkerMessage(msg: WorkerOutput): Promise<void> {
    switch (msg.type) {
      case "CollabReady": {
        // Worker has node ID and relay URL, create session record
        if (this.config.onSessionNeeded) {
          try {
            const sessionInfo: SessionInfo = {
              nodeId: msg.node_id,
              relayUrl: msg.relay_url,
            };
            this.sessionUri = await this.config.onSessionNeeded(sessionInfo);

            // Discover peers now that we have a session
            if (this.config.onPeersNeeded) {
              const peers = await this.config.onPeersNeeded(this.config.resourceUri);
              if (peers.length > 0) {
                this.addPeers(peers.map((p) => p.nodeId));
              }
            }
          } catch (err) {
            console.error("Failed to create session record:", err);
          }
        }
        break;
      }

      case "CollabJoined":
        // Successfully joined the gossip session
        break;

      case "RemoteUpdates": {
        // Apply remote Loro updates to main document
        const data = new Uint8Array(msg.data);
        this.inner.importUpdates(data);
        break;
      }

      case "PresenceUpdate": {
        // Store presence and render remote cursors
        const presence: PresenceSnapshot = {
          collaborators: msg.collaborators,
          peerCount: msg.peer_count,
        };
        this.currentPresence = presence;
        this.renderRemoteCursors();

        // Forward to callback
        this.config.onPresenceChanged?.(presence);
        break;
      }

      case "PeerConnected": {
        // A new peer connected, send our Join message with user info
        if (this.config.onUserInfoNeeded && this.workerBridge) {
          try {
            const userInfo = await this.config.onUserInfoNeeded();
            this.workerBridge.send({
              type: "BroadcastJoin",
              did: userInfo.did,
              display_name: userInfo.displayName,
            });
          } catch (err) {
            console.error("Failed to get user info for Join:", err);
          }
        }
        break;
      }

      case "CollabStopped":
        // Worker confirmed collab stopped
        break;

      case "Error":
        console.error("Worker error:", msg.message);
        break;

      case "Ready":
      case "Snapshot":
        // Handled elsewhere or not needed for collab
        break;
    }
  }

  // === Public API (same as Editor) ===

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

  // === Remote cursor positioning ===

  getCursorRectRelative(position: number): CursorRect | null {
    this.checkDestroyed();
    return this.inner.getCursorRectRelative(position) as CursorRect | null;
  }

  getSelectionRectsRelative(start: number, end: number): SelectionRect[] {
    this.checkDestroyed();
    return this.inner.getSelectionRectsRelative(start, end) as SelectionRect[];
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;

    // Stop collab if active (fire and forget)
    if (this.collabStarted && this.workerBridge) {
      this.workerBridge.send({ type: "StopCollab" });
      if (this.unsubscribeWorker) {
        this.unsubscribeWorker();
      }
      this.workerBridge.terminate();
    }

    this.detachEventListeners();
    this.inner.unmount();
    this.container = null;
    this.editorElement = null;
    this.workerBridge = null;
  }

  private checkDestroyed(): void {
    if (this.destroyed) {
      throw new Error("CollabEditor has been destroyed");
    }
  }
}
