

function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];

    heap[idx] = obj;
    return idx;
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
}

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

function dropObject(idx) {
    if (idx < 132) return;
    heap[idx] = heap_next;
    heap_next = idx;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(ptr, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function getObject(idx) { return heap[idx]; }

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        wasm.__wbindgen_export3(addHeapObject(e));
    }
}

let heap = new Array(128).fill(undefined);
heap.push(undefined, null, true, false);

let heap_next = heap.length;

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeObject(idx) {
    const ret = getObject(idx);
    dropObject(idx);
    return ret;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
function decodeText(ptr, len) {
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

let WASM_VECTOR_LEN = 0;

const JsMathResultFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_jsmathresult_free(ptr >>> 0, 1));

const JsResolvedContentFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_jsresolvedcontent_free(ptr >>> 0, 1));

/**
 * Result from rendering LaTeX math.
 */
export class JsMathResult {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(JsMathResult.prototype);
        obj.__wbg_ptr = ptr;
        JsMathResultFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        JsMathResultFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_jsmathresult_free(ptr, 0);
    }
    /**
     * @returns {boolean}
     */
    get success() {
        const ret = wasm.__wbg_get_jsmathresult_success(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {boolean} arg0
     */
    set success(arg0) {
        wasm.__wbg_set_jsmathresult_success(this.__wbg_ptr, arg0);
    }
    /**
     * @returns {string}
     */
    get html() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.__wbg_get_jsmathresult_html(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {string} arg0
     */
    set html(arg0) {
        const ptr0 = passStringToWasm0(arg0, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.__wbg_set_jsmathresult_html(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @returns {string | undefined}
     */
    get error() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.__wbg_get_jsmathresult_error(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            let v1;
            if (r0 !== 0) {
                v1 = getStringFromWasm0(r0, r1).slice();
                wasm.__wbindgen_export4(r0, r1 * 1, 1);
            }
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @param {string | null} [arg0]
     */
    set error(arg0) {
        var ptr0 = isLikeNone(arg0) ? 0 : passStringToWasm0(arg0, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        var len0 = WASM_VECTOR_LEN;
        wasm.__wbg_set_jsmathresult_error(this.__wbg_ptr, ptr0, len0);
    }
}
if (Symbol.dispose) JsMathResult.prototype[Symbol.dispose] = JsMathResult.prototype.free;

/**
 * Pre-rendered embed content for synchronous rendering.
 *
 * Build this by calling `create_resolved_content()` and adding embeds
 * with `resolved_content_add_embed()`.
 */
export class JsResolvedContent {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(JsResolvedContent.prototype);
        obj.__wbg_ptr = ptr;
        JsResolvedContentFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        JsResolvedContentFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_jsresolvedcontent_free(ptr, 0);
    }
    /**
     * Create an empty resolved content container.
     */
    constructor() {
        const ret = wasm.create_resolved_content();
        this.__wbg_ptr = ret >>> 0;
        JsResolvedContentFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Add pre-rendered embed HTML for an AT URI.
     *
     * # Arguments
     * * `at_uri` - The AT Protocol URI (e.g., "at://did:plc:.../app.bsky.feed.post/...")
     * * `html` - The pre-rendered HTML for this embed
     * @param {string} at_uri
     * @param {string} html
     */
    addEmbed(at_uri, html) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(at_uri, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(html, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            wasm.jsresolvedcontent_addEmbed(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
}
if (Symbol.dispose) JsResolvedContent.prototype[Symbol.dispose] = JsResolvedContent.prototype.free;

/**
 * Create an empty resolved content container.
 *
 * Use this to pre-render embeds before calling render functions.
 * @returns {JsResolvedContent}
 */
export function create_resolved_content() {
    const ret = wasm.create_resolved_content();
    return JsResolvedContent.__wrap(ret);
}

/**
 * Initialize panic hook for better error messages in console.
 */
export function init() {
    wasm.init();
}

/**
 * Render faceted text (rich text with mentions, links, etc.) to HTML.
 *
 * Accepts facets from several AT Protocol lexicons (app.bsky, pub.leaflet, blog.pckt).
 *
 * # Arguments
 * * `text` - The plain text content
 * * `facets_json` - Array of facets with `index` (byteStart/byteEnd) and `features` array
 * @param {string} text
 * @param {any} facets_json
 * @returns {string}
 */
export function render_faceted_text(text, facets_json) {
    let deferred3_0;
    let deferred3_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.render_faceted_text(retptr, ptr0, len0, addHeapObject(facets_json));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
        var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
        var ptr2 = r0;
        var len2 = r1;
        if (r3) {
            ptr2 = 0; len2 = 0;
            throw takeObject(r2);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Render markdown to HTML.
 *
 * # Arguments
 * * `markdown` - The markdown source text
 * * `resolved_content` - Optional pre-rendered embed content
 * @param {string} markdown
 * @param {JsResolvedContent | null} [resolved_content]
 * @returns {string}
 */
export function render_markdown(markdown, resolved_content) {
    let deferred4_0;
    let deferred4_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(markdown, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        let ptr1 = 0;
        if (!isLikeNone(resolved_content)) {
            _assertClass(resolved_content, JsResolvedContent);
            ptr1 = resolved_content.__destroy_into_raw();
        }
        wasm.render_markdown(retptr, ptr0, len0, ptr1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
        var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
        var ptr3 = r0;
        var len3 = r1;
        if (r3) {
            ptr3 = 0; len3 = 0;
            throw takeObject(r2);
        }
        deferred4_0 = ptr3;
        deferred4_1 = len3;
        return getStringFromWasm0(ptr3, len3);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred4_0, deferred4_1, 1);
    }
}

/**
 * Render LaTeX math to MathML.
 *
 * # Arguments
 * * `latex` - The LaTeX math expression
 * * `display_mode` - true for display math (block), false for inline math
 * @param {string} latex
 * @param {boolean} display_mode
 * @returns {JsMathResult}
 */
export function render_math(latex, display_mode) {
    const ptr0 = passStringToWasm0(latex, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.render_math(ptr0, len0, display_mode);
    return JsMathResult.__wrap(ret);
}

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
 * @param {string} at_uri
 * @param {any} record_json
 * @param {any | null} [fallback_author]
 * @param {JsResolvedContent | null} [resolved_content]
 * @returns {string}
 */
export function render_record(at_uri, record_json, fallback_author, resolved_content) {
    let deferred4_0;
    let deferred4_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(at_uri, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        let ptr1 = 0;
        if (!isLikeNone(resolved_content)) {
            _assertClass(resolved_content, JsResolvedContent);
            ptr1 = resolved_content.__destroy_into_raw();
        }
        wasm.render_record(retptr, ptr0, len0, addHeapObject(record_json), isLikeNone(fallback_author) ? 0 : addHeapObject(fallback_author), ptr1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
        var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
        var ptr3 = r0;
        var len3 = r1;
        if (r3) {
            ptr3 = 0; len3 = 0;
            throw takeObject(r2);
        }
        deferred4_0 = ptr3;
        deferred4_1 = len3;
        return getStringFromWasm0(ptr3, len3);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred4_0, deferred4_1, 1);
    }
}

const imports = {
    __wbindgen_placeholder__: {
        __wbg_Error_52673b7de5a0ca89: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return addHeapObject(ret);
        },
        __wbg_Number_2d1dcfcf4ec51736: function(arg0) {
            const ret = Number(getObject(arg0));
            return ret;
        },
        __wbg_String_8f0eb39a4a4c2f66: function(arg0, arg1) {
            const ret = String(getObject(arg1));
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_bigint_get_as_i64_6e32f5e6aff02e1d: function(arg0, arg1) {
            const v = getObject(arg1);
            const ret = typeof(v) === 'bigint' ? v : undefined;
            getDataViewMemory0().setBigInt64(arg0 + 8 * 1, isLikeNone(ret) ? BigInt(0) : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_boolean_get_dea25b33882b895b: function(arg0) {
            const v = getObject(arg0);
            const ret = typeof(v) === 'boolean' ? v : undefined;
            return isLikeNone(ret) ? 0xFFFFFF : ret ? 1 : 0;
        },
        __wbg___wbindgen_debug_string_adfb662ae34724b6: function(arg0, arg1) {
            const ret = debugString(getObject(arg1));
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_in_0d3e1e8f0c669317: function(arg0, arg1) {
            const ret = getObject(arg0) in getObject(arg1);
            return ret;
        },
        __wbg___wbindgen_is_bigint_0e1a2e3f55cfae27: function(arg0) {
            const ret = typeof(getObject(arg0)) === 'bigint';
            return ret;
        },
        __wbg___wbindgen_is_function_8d400b8b1af978cd: function(arg0) {
            const ret = typeof(getObject(arg0)) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_object_ce774f3490692386: function(arg0) {
            const val = getObject(arg0);
            const ret = typeof(val) === 'object' && val !== null;
            return ret;
        },
        __wbg___wbindgen_is_undefined_f6b95eab589e0269: function(arg0) {
            const ret = getObject(arg0) === undefined;
            return ret;
        },
        __wbg___wbindgen_jsval_eq_b6101cc9cef1fe36: function(arg0, arg1) {
            const ret = getObject(arg0) === getObject(arg1);
            return ret;
        },
        __wbg___wbindgen_jsval_loose_eq_766057600fdd1b0d: function(arg0, arg1) {
            const ret = getObject(arg0) == getObject(arg1);
            return ret;
        },
        __wbg___wbindgen_number_get_9619185a74197f95: function(arg0, arg1) {
            const obj = getObject(arg1);
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_a2a31e16edf96e42: function(arg0, arg1) {
            const obj = getObject(arg1);
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_dd24417ed36fc46e: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_call_abb4ff46ce38be40: function() { return handleError(function (arg0, arg1) {
            const ret = getObject(arg0).call(getObject(arg1));
            return addHeapObject(ret);
        }, arguments) },
        __wbg_done_62ea16af4ce34b24: function(arg0) {
            const ret = getObject(arg0).done;
            return ret;
        },
        __wbg_entries_83c79938054e065f: function(arg0) {
            const ret = Object.entries(getObject(arg0));
            return addHeapObject(ret);
        },
        __wbg_error_7534b8e9a36f1ab4: function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_export4(deferred0_0, deferred0_1, 1);
            }
        },
        __wbg_get_6b7bd52aca3f9671: function(arg0, arg1) {
            const ret = getObject(arg0)[arg1 >>> 0];
            return addHeapObject(ret);
        },
        __wbg_get_af9dab7e9603ea93: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(getObject(arg0), getObject(arg1));
            return addHeapObject(ret);
        }, arguments) },
        __wbg_get_with_ref_key_1dc361bd10053bfe: function(arg0, arg1) {
            const ret = getObject(arg0)[getObject(arg1)];
            return addHeapObject(ret);
        },
        __wbg_instanceof_ArrayBuffer_f3320d2419cd0355: function(arg0) {
            let result;
            try {
                result = getObject(arg0) instanceof ArrayBuffer;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Map_084be8da74364158: function(arg0) {
            let result;
            try {
                result = getObject(arg0) instanceof Map;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Uint8Array_da54ccc9d3e09434: function(arg0) {
            let result;
            try {
                result = getObject(arg0) instanceof Uint8Array;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_isArray_51fd9e6422c0a395: function(arg0) {
            const ret = Array.isArray(getObject(arg0));
            return ret;
        },
        __wbg_isSafeInteger_ae7d3f054d55fa16: function(arg0) {
            const ret = Number.isSafeInteger(getObject(arg0));
            return ret;
        },
        __wbg_iterator_27b7c8b35ab3e86b: function() {
            const ret = Symbol.iterator;
            return addHeapObject(ret);
        },
        __wbg_length_22ac23eaec9d8053: function(arg0) {
            const ret = getObject(arg0).length;
            return ret;
        },
        __wbg_length_d45040a40c570362: function(arg0) {
            const ret = getObject(arg0).length;
            return ret;
        },
        __wbg_new_6421f6084cc5bc5a: function(arg0) {
            const ret = new Uint8Array(getObject(arg0));
            return addHeapObject(ret);
        },
        __wbg_new_8a6f238a6ece86ea: function() {
            const ret = new Error();
            return addHeapObject(ret);
        },
        __wbg_next_138a17bbf04e926c: function(arg0) {
            const ret = getObject(arg0).next;
            return addHeapObject(ret);
        },
        __wbg_next_3cfe5c0fe2a4cc53: function() { return handleError(function (arg0) {
            const ret = getObject(arg0).next();
            return addHeapObject(ret);
        }, arguments) },
        __wbg_prototypesetcall_dfe9b766cdc1f1fd: function(arg0, arg1, arg2) {
            Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), getObject(arg2));
        },
        __wbg_stack_0ed75d68575b0f3c: function(arg0, arg1) {
            const ret = getObject(arg1).stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_value_57b7b035e117f7ee: function(arg0) {
            const ret = getObject(arg0).value;
            return addHeapObject(ret);
        },
        __wbindgen_cast_2241b6af4c4b2941: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return addHeapObject(ret);
        },
        __wbindgen_cast_4625c577ab2ec9ee: function(arg0) {
            // Cast intrinsic for `U64 -> Externref`.
            const ret = BigInt.asUintN(64, arg0);
            return addHeapObject(ret);
        },
        __wbindgen_cast_9ae0607507abb057: function(arg0) {
            // Cast intrinsic for `I64 -> Externref`.
            const ret = arg0;
            return addHeapObject(ret);
        },
        __wbindgen_object_clone_ref: function(arg0) {
            const ret = getObject(arg0);
            return addHeapObject(ret);
        },
        __wbindgen_object_drop_ref: function(arg0) {
            takeObject(arg0);
        },
        __wbindgen_object_is_undefined: function(arg0) {
            const ret = getObject(arg0) === undefined;
            return ret;
        },
    },

};

const wasmUrl = new URL('weaver_renderer_bg.wasm', import.meta.url);
const wasm = (await WebAssembly.instantiateStreaming(fetch(wasmUrl), imports)).instance.exports;
export { wasm as __wasm };

wasm.__wbindgen_start();
