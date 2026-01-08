/**
 * TypeScript types for weaver-editor.
 *
 * These types match the Rust types exposed via wasm-bindgen/tsify.
 */

/** Pending image waiting for upload. */
export interface PendingImage {
  localId: string;
  data: Uint8Array;
  mimeType: string;
  name: string;
}

/** Finalized image with blob ref and staging URI. */
export interface FinalizedImage {
  blobRef: BlobRef;
  stagingUri: string;
}

/** AT Protocol blob reference. */
export interface BlobRef {
  $type: "blob";
  ref: { $link: string };
  mimeType: string;
  size: number;
}

/** Aspect ratio for images/videos. */
export interface AspectRatio {
  width: number;
  height: number;
}

/** Image embed in entry. */
export interface ImageEmbed {
  image: BlobRef;
  alt: string;
  aspectRatio?: AspectRatio;
}

/** Record embed (strong ref). */
export interface RecordEmbed {
  uri: string;
  cid: string;
}

/** External link embed. */
export interface ExternalEmbed {
  uri: string;
  title: string;
  description: string;
  thumb?: BlobRef;
}

/** Video embed. */
export interface VideoEmbed {
  video: BlobRef;
  alt?: string;
  aspectRatio?: AspectRatio;
}

/** Entry embeds container. */
export interface EntryEmbeds {
  images?: { images: ImageEmbed[] };
  records?: { records: RecordEmbed[] };
  externals?: { externals: ExternalEmbed[] };
  videos?: { videos: VideoEmbed[] };
}

/** Author reference. */
export interface Author {
  did: string;
}

/** Entry JSON matching sh.weaver.notebook.entry lexicon. */
export interface EntryJson {
  title: string;
  path: string;
  content: string;
  createdAt: string;
  updatedAt?: string;
  tags?: string[];
  embeds?: EntryEmbeds;
  authors?: Author[];
  contentWarnings?: string[];
  rating?: string;
}

/** Selection range in the editor. */
export interface Selection {
  anchor: number;
  head: number;
}

/** Cursor rectangle for positioning. */
export interface CursorRect {
  x: number;
  y: number;
  height: number;
}

/** Selection rectangle for highlighting. */
export interface SelectionRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Rendered paragraph data. */
export interface ParagraphRender {
  id: string;
  html: string;
  charStart: number;
  charEnd: number;
}

/** Result of event handling. */
export type EventResult = "Handled" | "PassThrough" | "HandledAsync";

/** Editor action types. */
export type EditorAction =
  | { type: "insert"; text: string; start: number; end: number }
  | { type: "delete"; start: number; end: number }
  | { type: "insertParagraph"; start: number; end: number }
  | { type: "undo" }
  | { type: "redo" }
  | { type: "bold"; start: number; end: number }
  | { type: "italic"; start: number; end: number }
  | { type: "strikethrough"; start: number; end: number }
  | { type: "code"; start: number; end: number }
  | { type: "link"; url: string; start: number; end: number }
  | { type: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; start: number; end: number }
  | { type: "bulletList"; start: number; end: number }
  | { type: "numberedList"; start: number; end: number }
  | { type: "blockquote"; start: number; end: number }
  | { type: "codeBlock"; language?: string; start: number; end: number };

/** Configuration for creating an editor. */
export interface EditorConfig {
  /** Container element to mount the editor in. */
  container: HTMLElement;

  /** Initial markdown content. */
  initialMarkdown?: string;

  /** Initial snapshot (EntryJson). */
  initialSnapshot?: EntryJson;

  /** Pre-resolved embed content. */
  resolvedContent?: ResolvedContent;

  /** Called after each edit. */
  onChange?: () => void;

  /** Called when user adds an image. */
  onImageAdd?: (image: PendingImage) => void;
}

/** Pre-resolved embed content for initial load. */
export interface ResolvedContent {
  /** Map of AT URI -> rendered HTML. */
  embeds: Map<string, string>;
}

/** Session info for collab (from worker). */
export interface SessionInfo {
  nodeId: string;
  relayUrl: string | null;
}

/** Peer info for collab. */
export interface PeerInfo {
  nodeId: string;
  did?: string;
  displayName?: string;
}

/** Collaborator presence info. */
export interface CollaboratorInfo {
  nodeId: string;
  did: string;
  displayName: string;
  color: number;
  cursorPosition?: number;
  selection?: [number, number];
}

/** Presence state snapshot. */
export interface PresenceSnapshot {
  collaborators: CollaboratorInfo[];
  peerCount: number;
}

/** User info for collab presence. */
export interface UserInfo {
  did: string;
  displayName: string;
}

/** Configuration for creating a collab editor. */
export interface CollabEditorConfig extends EditorConfig {
  /** Resource URI (AT URI of entry/draft being edited). */
  resourceUri: string;

  /** Initial Loro snapshot bytes (optional). */
  initialLoroSnapshot?: Uint8Array;

  /** Called when a session record needs to be created on PDS. */
  onSessionNeeded?: (session: SessionInfo) => Promise<string>;

  /** Called to refresh session record periodically. */
  onSessionRefresh?: (sessionUri: string) => Promise<void>;

  /** Called when session ends (delete record). */
  onSessionEnd?: (sessionUri: string) => Promise<void>;

  /** Called to discover peers from PDS/index. */
  onPeersNeeded?: (resourceUri: string) => Promise<PeerInfo[]>;

  /** Called when presence state changes. */
  onPresenceChanged?: (presence: PresenceSnapshot) => void;

  /** Called to get current user info for presence announcements. */
  onUserInfoNeeded?: () => Promise<UserInfo>;
}

/** Collab editor interface (extends Editor). */
export interface CollabEditor extends Editor {
  // Loro sync
  exportSnapshot(): Uint8Array;
  exportUpdatesSince(version: Uint8Array): Uint8Array | null;
  importUpdates(data: Uint8Array): void;
  getVersion(): Uint8Array;

  // Collab info
  getCollabTopic(): Uint8Array | null;
  getResourceUri(): string;

  // Collab lifecycle
  startCollab(bootstrapPeers?: string[]): Promise<void>;
  stopCollab(): Promise<void>;
  addPeers(nodeIds: string[]): void;

  // Remote cursor positioning
  getCursorRectRelative(position: number): CursorRect | null;
  getSelectionRectsRelative(start: number, end: number): SelectionRect[];
}

/** Editor interface. */
export interface Editor {
  // Content access
  getMarkdown(): string;
  getSnapshot(): EntryJson;
  toEntry(): EntryJson;

  // Metadata
  getTitle(): string;
  setTitle(title: string): void;
  getPath(): string;
  setPath(path: string): void;
  getTags(): string[];
  setTags(tags: string[]): void;

  // Actions
  executeAction(action: EditorAction): void;

  // Image handling
  addPendingImage(image: PendingImage, dataUrl: string): void;
  finalizeImage(
    localId: string,
    finalized: FinalizedImage,
    blobRkey: string,
    identifier: string
  ): void;
  removeImage(localId: string): void;
  getPendingImages(): PendingImage[];
  getStagingUris(): string[];

  // Entry index (for wikilinks)
  addEntryToIndex(title: string, path: string, canonicalUrl: string): void;
  clearEntryIndex(): void;

  // Cursor/selection
  getCursorOffset(): number;
  setCursorOffset(offset: number): void;
  getLength(): number;

  // Undo/redo
  canUndo(): boolean;
  canRedo(): boolean;

  // Lifecycle
  focus(): void;
  blur(): void;
  destroy(): void;

  // Rendering
  getParagraphs(): ParagraphRender[];
  renderAndUpdateDom(): void;
}
