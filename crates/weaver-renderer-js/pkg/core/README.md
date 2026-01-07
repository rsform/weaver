# @weaver/renderer

WASM bindings for rendering AT Protocol records (Bluesky posts, etc.) to HTML.

## Installation

```bash
npm install @weaver.sh/renderer-full   # With syntax highlighting
npm install @weaver.sh/renderer-core   # Light(er) weight
```

## Usage

### With a bundler (webpack, vite, etc.)

```javascript
import init, { render_record, render_markdown } from '@weaver/renderer-full';

await init();

const html = render_record(atUri, recordJson);
```

### Direct browser usage (no bundler)

```html
<script type="module">
  import init, { render_record } from '@weaver/renderer-full/web';
  await init();
  // ...
</script>
```

### Node.js

```javascript
const { render_record } = require('@weaver/renderer-full/nodejs');
```

## API

- `render_record(at_uri, record_json, fallback_author?, resolved_content?)` - Render an AT Protocol record
- `render_markdown(markdown, resolved_content?)` - Render markdown to HTML
- `render_math(latex, display_mode)` - Render LaTeX math to MathML
- `render_faceted_text(text, facets_json)` - Render rich text with facets
