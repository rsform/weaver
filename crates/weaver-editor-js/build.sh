#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PKG_NAME="@weaver.sh/editor"
PKG_VERSION="0.1.1"

# Targets to build
TARGETS=(bundler web nodejs deno)

COMMAND="${1:-build}"
shift || true

# Feature variants
declare -A VARIANTS=(
    ["core"]=""
    ["collab"]="collab"
)

build() {
    local target="$1"
    local variant="$2"
    local features="$3"
    local out_dir="pkg/${variant}/${target}"

    echo "Building ${variant}/${target}..."

    local feature_args="--no-default-features"
    if [[ -n "$features" ]]; then
        feature_args="$feature_args --features $features"
    fi

    wasm-pack build \
        --out-name weaver_editor \
        --out-dir "$out_dir" \
        --target "$target" \
        $feature_args

    # Report size
    local wasm_file="${out_dir}/weaver_editor_bg.wasm"
    if [[ -f "$wasm_file" ]]; then
        local size=$(ls -lh "$wasm_file" | awk '{print $5}')
        echo "  → ${size}"
    fi
}

generate_package_json() {
    local variant="$1"
    local out_dir="pkg/${variant}"
    local pkg_suffix=""
    local description=""

    if [[ "$variant" == "collab" ]]; then
        pkg_suffix="-collab"
        description="Weaver markdown editor with collaborative editing (Loro CRDT + iroh P2P)"
    else
        pkg_suffix="-core"
        description="Weaver markdown editor (local editing, lightweight)"
    fi

    # Worker export only for collab variant
    local worker_export=""
    local worker_files=""
    if [[ "$variant" == "collab" ]]; then
        worker_export=',
    "./worker": {
      "import": "./worker/editor_worker.js"
    }'
        worker_files=',
    "worker/"'
    fi

    cat > "${out_dir}/package.json" << EOF
{
  "name": "${PKG_NAME}${pkg_suffix}",
  "version": "${PKG_VERSION}",
  "description": "${description}",
  "license": "MPL-2.0",
  "repository": {
    "type": "git",
    "url": "https://tangled.org/nonbinary.computer/weaver"
  },
  "keywords": ["atproto", "markdown", "editor", "wasm", "weaver"],
  "main": "index.js",
  "module": "index.js",
  "types": "index.d.ts",
  "exports": {
    ".": {
      "import": "./index.js",
      "types": "./index.d.ts"
    },
    "./types": {
      "import": "./types.js",
      "types": "./types.d.ts"
    },
    "./wasm/bundler": {
      "import": "./bundler/weaver_editor.js",
      "types": "./bundler/weaver_editor.d.ts"
    },
    "./wasm/web": {
      "import": "./web/weaver_editor.js",
      "types": "./web/weaver_editor.d.ts"
    },
    "./wasm/nodejs": {
      "import": "./nodejs/weaver_editor.js",
      "require": "./nodejs/weaver_editor.js",
      "types": "./nodejs/weaver_editor.d.ts"
    },
    "./wasm/deno": {
      "import": "./deno/weaver_editor.js",
      "types": "./deno/weaver_editor.d.ts"
    },
    "./weaver-editor.css": "./weaver-editor.css"${worker_export}
  },
  "files": [
    "index.js",
    "index.d.ts",
    "types.js",
    "types.d.ts",
    "weaver-editor.css",
    "bundler/",
    "web/",
    "nodejs/",
    "deno/",
    "README.md"${worker_files}
  ]
}
EOF
}

generate_readme() {
    local variant="$1"
    local out_dir="pkg/${variant}"

    cat > "${out_dir}/README.md" << 'EOF'
# @weaver.sh/editor

WASM-based markdown editor for weaver.sh.

## Installation

```bash
npm install @weaver.sh/editor-core     # Local editing only
npm install @weaver.sh/editor-collab   # With collaborative editing
```

## Usage

### With a bundler (webpack, vite, etc.)

```javascript
import init, { JsEditor } from '@weaver.sh/editor-core';

await init();

const editor = JsEditor.fromMarkdown('# Hello\n\nWorld');
console.log(editor.getMarkdown());
```

### Direct browser usage (no bundler)

```html
<script type="module">
  import init, { JsEditor } from '@weaver.sh/editor-core/web';
  await init();
  // ...
</script>
```

### Node.js

```javascript
const { JsEditor } = require('@weaver.sh/editor-core/nodejs');
```

## API

See the TypeScript definitions for full API documentation.

### Core

- `JsEditor.new()` - Create empty editor
- `JsEditor.fromMarkdown(content)` - Create from markdown
- `JsEditor.fromSnapshot(entry)` - Create from EntryJson snapshot
- `editor.getMarkdown()` - Get markdown content
- `editor.getSnapshot()` - Get EntryJson for drafts
- `editor.toEntry()` - Get validated EntryJson for publishing
- `editor.executeAction(action)` - Execute an EditorAction
- `editor.setTitle(title)` / `editor.setPath(path)` / `editor.setTags(tags)`

### Images

- `editor.addPendingImage(image)` - Track pending upload
- `editor.finalizeImage(localId, finalized)` - Mark upload complete
- `editor.getPendingImages()` - Get images awaiting upload
- `editor.getStagingUris()` - Get staging record URIs for cleanup

### Collab (editor-collab only)

- `JsCollabEditor` - Collaborative editor with Loro CRDT
- `editor.exportUpdates()` / `editor.importUpdates(bytes)`
- `editor.addPeer(nodeId)` / `editor.removePeer(nodeId)`
EOF
}

build_worker() {
    echo "Building editor worker WASM..."

    # Build the worker binary from weaver-editor-crdt
    # Must be in workspace root for cargo to find the crate
    local workspace_root="$(cd ../.. && pwd)"

    export RUSTFLAGS='--cfg getrandom_backend="wasm_js"'

    (cd "$workspace_root" && cargo build \
        -p weaver-editor-crdt \
        --bin editor_worker \
        --target wasm32-unknown-unknown \
        --release \
        --features collab)

    # Create worker output directory
    local worker_out="pkg/collab/worker"
    mkdir -p "$worker_out"

    # Run wasm-bindgen with no-modules target for web worker compatibility
    wasm-bindgen \
        "$workspace_root/target/wasm32-unknown-unknown/release/editor_worker.wasm" \
        --out-dir "$worker_out" \
        --target no-modules \
        --no-typescript

    # Report size
    local wasm_file="${worker_out}/editor_worker_bg.wasm"
    if [[ -f "$wasm_file" ]]; then
        local size=$(ls -lh "$wasm_file" | awk '{print $5}')
        echo "  → Worker WASM: ${size}"
    fi
}

build_typescript() {
    echo "Building TypeScript wrapper..."

    # Install deps if needed
    if [[ ! -d "ts/node_modules" ]]; then
        (cd ts && npm install)
    fi

    # Link WASM output so TypeScript can find it during compilation
    # Use collab/bundler as source - it has all exports (JsCollabEditor + JsEditor)
    # Core variant users who import collab will get runtime error, which is expected
    rm -rf ts/bundler
    ln -s ../pkg/collab/bundler ts/bundler

    # Compile TypeScript
    (cd ts && npm run build)

    # Copy to pkg variants
    for variant in "${!VARIANTS[@]}"; do
        local out_dir="pkg/${variant}"

        # Copy compiled JS/TS
        cp -r ts/dist/* "${out_dir}/"

        # Copy CSS
        cp ts/weaver-editor.css "${out_dir}/"
    done

    echo "  → TypeScript wrapper built"
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

    # Build worker WASM for collab variant
    build_worker

    # Build TypeScript wrapper
    build_typescript

    echo ""
    echo "Build complete!"
    echo ""
    echo "Editor WASM:"
    ls -lh pkg/core/web/*.wasm pkg/collab/web/*.wasm 2>/dev/null || true
    echo ""
    echo "Worker WASM (collab only):"
    ls -lh pkg/collab/worker/*.wasm 2>/dev/null || true
    echo ""
    echo "Packages:"
    echo "  pkg/core/   - @weaver.sh/editor-core (local editing)"
    echo "  pkg/collab/ - @weaver.sh/editor-collab (with CRDT collab)"
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
