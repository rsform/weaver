/* tslint:disable */
/* eslint-disable */

export class JsMathResult {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  success: boolean;
  html: string;
  get error(): string | undefined;
  set error(value: string | null | undefined);
}

export class JsResolvedContent {
  free(): void;
  [Symbol.dispose](): void;
  /**
   * Create an empty resolved content container.
   */
  constructor();
  /**
   * Add pre-rendered embed HTML for an AT URI.
   *
   * # Arguments
   * * `at_uri` - The AT Protocol URI (e.g., "at://did:plc:.../app.bsky.feed.post/...")
   * * `html` - The pre-rendered HTML for this embed
   */
  addEmbed(at_uri: string, html: string): void;
}

/**
 * Create an empty resolved content container.
 *
 * Use this to pre-render embeds before calling render functions.
 */
export function create_resolved_content(): JsResolvedContent;

/**
 * Initialize panic hook for better error messages in console.
 */
export function init(): void;

/**
 * Render faceted text (rich text with mentions, links, etc.) to HTML.
 *
 * Accepts facets from several AT Protocol lexicons (app.bsky, pub.leaflet, blog.pckt).
 *
 * # Arguments
 * * `text` - The plain text content
 * * `facets_json` - Array of facets with `index` (byteStart/byteEnd) and `features` array
 */
export function render_faceted_text(text: string, facets_json: any): string;

/**
 * Render markdown to HTML.
 *
 * # Arguments
 * * `markdown` - The markdown source text
 * * `resolved_content` - Optional pre-rendered embed content
 */
export function render_markdown(markdown: string, resolved_content?: JsResolvedContent | null): string;

/**
 * Render LaTeX math to MathML.
 *
 * # Arguments
 * * `latex` - The LaTeX math expression
 * * `display_mode` - true for display math (block), false for inline math
 */
export function render_math(latex: string, display_mode: boolean): JsMathResult;

/**
 * Render an AT Protocol record as HTML.
 *
 * Takes a record URI and the record data (typically fetched from an appview).
 * Returns the rendered HTML string.
 *
 * # Arguments
 * * `at_uri` - The AT Protocol URI (e.g., "at://did:plc:.../app.bsky.feed.post/...")
 * * `record_json` - The record data as JSON
 * * `fallback_author` - Optional author profile for records that don't include author info
 * * `resolved_content` - Optional pre-rendered embed content
 */
export function render_record(at_uri: string, record_json: any, fallback_author?: any | null, resolved_content?: JsResolvedContent | null): string;
