#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PKG_NAME="@weaver.sh/renderer"
PKG_VERSION="0.1.1"

# Targets to build
TARGETS=(bundler web nodejs deno)

COMMAND="${1:-build}"
shift || true

# Feature variants
declare -A VARIANTS=(
    ["core"]=""
    ["full"]="syntax-highlighting"
)

build() {
    local target="$1"
    local variant="$2"
    local features="$3"
    local out_dir="pkg/${variant}/${target}"

    echo "Building ${variant}/${target}..."

    local feature_args=""
    if [[ -n "$features" ]]; then
        feature_args="--features $features"
    fi

    wasm-pack build \
        --out-name weaver_renderer \
        --out-dir "$out_dir" \
        --target "$target" \
        --no-default-features \
        $feature_args

    # Report size
    local wasm_file="${out_dir}/weaver_renderer_bg.wasm"
    if [[ -f "$wasm_file" ]]; then
        local size=$(ls -lh "$wasm_file" | awk '{print $5}')
        echo "  → ${size}"
    fi
}

generate_package_json() {
    local variant="$1"
    local out_dir="pkg/${variant}"
    local description="AT Protocol record renderer (${variant})"

    if [[ "$variant" == "full" ]]; then
        description="AT Protocol record renderer with syntax highlighting"
    else
        description="AT Protocol record renderer (lightweight, no syntax highlighting)"
    fi

    cat > "${out_dir}/package.json" << EOF
{
  "name": "${PKG_NAME}-${variant}",
  "version": "${PKG_VERSION}",
  "description": "${description}",
  "license": "MPL-2.0",
  "repository": {
    "type": "git",
    "url": "https://tangled.org/nonbinary.computer/weaver"
  },
  "keywords": ["atproto", "bluesky", "markdown", "renderer", "wasm"],
  "main": "nodejs/weaver_renderer.js",
  "module": "bundler/weaver_renderer.js",
  "browser": "web/weaver_renderer.js",
  "types": "bundler/weaver_renderer.d.ts",
  "exports": {
    ".": {
      "deno": "./deno/weaver_renderer.js",
      "node": {
        "import": "./nodejs/weaver_renderer.js",
        "require": "./nodejs/weaver_renderer.js"
      },
      "browser": {
        "import": "./web/weaver_renderer.js"
      },
      "default": "./bundler/weaver_renderer.js"
    },
    "./bundler": {
      "import": "./bundler/weaver_renderer.js",
      "types": "./bundler/weaver_renderer.d.ts"
    },
    "./web": {
      "import": "./web/weaver_renderer.js",
      "types": "./web/weaver_renderer.d.ts"
    },
    "./nodejs": {
      "import": "./nodejs/weaver_renderer.js",
      "require": "./nodejs/weaver_renderer.js",
      "types": "./nodejs/weaver_renderer.d.ts"
    },
    "./deno": {
      "import": "./deno/weaver_renderer.js",
      "types": "./deno/weaver_renderer.d.ts"
    }
  },
  "files": [
    "bundler/",
    "web/",
    "nodejs/",
    "deno/",
    "README.md"
  ]
}
EOF
}

generate_readme() {
    local variant="$1"
    local out_dir="pkg/${variant}"

    cat > "${out_dir}/README.md" << 'EOF'
# @weaver.sh/renderer

WASM bindings for rendering AT Protocol records (Bluesky posts, etc.) to HTML.

## Installation

```bash
npm install @weaver.sh/renderer-full   # With syntax highlighting
npm install @weaver.sh/renderer-core   # Light(er) weight
```

## Usage

### With a bundler (webpack, vite, etc.)

```javascript
import init, { render_record, render_markdown } from '@weaver.sh/renderer-full';

await init();

const html = render_record(atUri, recordJson);
```

### Direct browser usage (no bundler)

```html
<script type="module">
  import init, { render_record } from '@weaver.sh/renderer-full/web';
  await init();
  // ...
</script>
```

### Node.js

```javascript
const { render_record } = require('@weaver.sh/renderer-full/nodejs');
```

## API

- `render_record(at_uri, record_json, fallback_author?, resolved_content?)` - Render an AT Protocol record
- `render_markdown(markdown, resolved_content?)` - Render markdown to HTML
- `render_math(latex, display_mode)` - Render LaTeX math to MathML
- `render_faceted_text(text, facets_json)` - Render rich text with facets
EOF
}

do_build() {
# Clean previous builds
rm -rf pkg

# Build all combinations
for variant in "${!VARIANTS[@]}"; do
    features="${VARIANTS[$variant]}"

    for target in "${TARGETS[@]}"; do
        build "$target" "$variant" "$features"
    done

    generate_package_json "$variant"
    generate_readme "$variant"

    # Clean up wasm-pack artifacts we don't need
    find "pkg/${variant}" -name ".gitignore" -delete
    find "pkg/${variant}" -name "package.json" -path "*/bundler/*" -delete
    find "pkg/${variant}" -name "package.json" -path "*/web/*" -delete
    find "pkg/${variant}" -name "package.json" -path "*/nodejs/*" -delete
    find "pkg/${variant}" -name "package.json" -path "*/deno/*" -delete
done

echo ""
echo "Build complete!"
echo ""
ls -lh pkg/core/web/*.wasm pkg/full/web/*.wasm 2>/dev/null || true
echo ""
echo "Packages:"
echo "  pkg/core/ - @weaver.sh/renderer-core (no syntax highlighting)"
echo "  pkg/full/ - @weaver.sh/renderer-full (with syntax highlighting)"
}

do_pack() {
    echo "Packing..."
    for variant in "${!VARIANTS[@]}"; do
        echo "  ${variant}..."
        (cd "pkg/${variant}" && npm pack)
    done
    echo ""
    echo "Tarballs created:"
    ls -lh pkg/*/*.tgz 2>/dev/null || true
}

do_publish() {
    local tag="${1:-}"
    local tag_arg=""
    if [[ -n "$tag" ]]; then
        tag_arg="--tag $tag"
    fi

    echo "Publishing..."
    for variant in "${!VARIANTS[@]}"; do
        echo "  ${variant}..."
        (cd "pkg/${variant}" && npm publish --access public $tag_arg)
    done
    echo ""
    echo "Published!"
}

usage() {
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  build    Build all variants and targets (default)"
    echo "  pack     Create npm tarballs"
    echo "  publish  Publish to npm registry"
    echo "  all      Build, pack, and publish"
    echo ""
    echo "Options for publish:"
    echo "  --tag <tag>  Publish with a specific tag (e.g., 'next', 'beta')"
}

case "$COMMAND" in
    build)
        do_build
        ;;
    pack)
        do_pack
        ;;
    publish)
        do_publish "$@"
        ;;
    all)
        do_build
        do_pack
        do_publish "$@"
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        echo "Unknown command: $COMMAND"
        usage
        exit 1
        ;;
esac
