let wasm_bindgen;
(function() {
    const __exports = {};
    let script_src;
    if (typeof document !== 'undefined' && document.currentScript !== null) {
        script_src = new URL(document.currentScript.src, location.href).toString();
    }
    let wasm = undefined;

    function addToExternrefTable0(obj) {
        const idx = wasm.__externref_table_alloc();
        wasm.__wbindgen_externrefs.set(idx, obj);
        return idx;
    }

    const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(state => state.dtor(state.a, state.b));

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

    function handleError(f, args) {
        try {
            return f.apply(this, args);
        } catch (e) {
            const idx = addToExternrefTable0(e);
            wasm.__wbindgen_exn_store(idx);
        }
    }

    function isLikeNone(x) {
        return x === undefined || x === null;
    }

    function makeClosure(arg0, arg1, dtor, f) {
        const state = { a: arg0, b: arg1, cnt: 1, dtor };
        const real = (...args) => {

            // First up with a closure we increment the internal reference
            // count. This ensures that the Rust closure environment won't
            // be deallocated while we're invoking it.
            state.cnt++;
            try {
                return f(state.a, state.b, ...args);
            } finally {
                real._wbg_cb_unref();
            }
        };
        real._wbg_cb_unref = () => {
            if (--state.cnt === 0) {
                state.dtor(state.a, state.b);
                state.a = 0;
                CLOSURE_DTORS.unregister(state);
            }
        };
        CLOSURE_DTORS.register(real, state, state);
        return real;
    }

    function makeMutClosure(arg0, arg1, dtor, f) {
        const state = { a: arg0, b: arg1, cnt: 1, dtor };
        const real = (...args) => {

            // First up with a closure we increment the internal reference
            // count. This ensures that the Rust closure environment won't
            // be deallocated while we're invoking it.
            state.cnt++;
            const a = state.a;
            state.a = 0;
            try {
                return f(a, state.b, ...args);
            } finally {
                state.a = a;
                real._wbg_cb_unref();
            }
        };
        real._wbg_cb_unref = () => {
            if (--state.cnt === 0) {
                state.dtor(state.a, state.b);
                state.a = 0;
                CLOSURE_DTORS.unregister(state);
            }
        };
        CLOSURE_DTORS.register(real, state, state);
        return real;
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

    let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
    cachedTextDecoder.decode();
    function decodeText(ptr, len) {
        return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
    }

    const cachedTextEncoder = new TextEncoder();

    if (!('encodeInto' in cachedTextEncoder)) {
        cachedTextEncoder.encodeInto = function (arg, view) {
            const buf = cachedTextEncoder.encode(arg);
            view.set(buf);
            return {
                read: arg.length,
                written: buf.length
            };
        }
    }

    let WASM_VECTOR_LEN = 0;

    function wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke_______1_(arg0, arg1) {
        wasm.wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke_______1_(arg0, arg1);
    }

    function wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke___web_sys_8b37e013ecf136cb___features__gen_MessageEvent__MessageEvent_____(arg0, arg1, arg2) {
        wasm.wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke___web_sys_8b37e013ecf136cb___features__gen_MessageEvent__MessageEvent_____(arg0, arg1, arg2);
    }

    function wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke______(arg0, arg1) {
        wasm.wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke______(arg0, arg1);
    }

    function wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke___wasm_bindgen_1add006a0ed82fd3___JsValue_____(arg0, arg1, arg2) {
        wasm.wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke___wasm_bindgen_1add006a0ed82fd3___JsValue_____(arg0, arg1, arg2);
    }

    const __wbindgen_enum_RequestCache = ["default", "no-store", "reload", "no-cache", "force-cache", "only-if-cached"];

    const __wbindgen_enum_RequestCredentials = ["omit", "same-origin", "include"];

    const __wbindgen_enum_RequestMode = ["same-origin", "no-cors", "cors", "navigate"];

    const JSOwnerFinalization = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(ptr => wasm.__wbg_jsowner_free(ptr >>> 0, 1));

    class JSOwner {
        __destroy_into_raw() {
            const ptr = this.__wbg_ptr;
            this.__wbg_ptr = 0;
            JSOwnerFinalization.unregister(this);
            return ptr;
        }
        free() {
            const ptr = this.__destroy_into_raw();
            wasm.__wbg_jsowner_free(ptr, 0);
        }
    }
    if (Symbol.dispose) JSOwner.prototype[Symbol.dispose] = JSOwner.prototype.free;
    __exports.JSOwner = JSOwner;

    const EXPECTED_RESPONSE_TYPES = new Set(['basic', 'cors', 'default']);

    async function __wbg_load(module, imports) {
        if (typeof Response === 'function' && module instanceof Response) {
            if (typeof WebAssembly.instantiateStreaming === 'function') {
                try {
                    return await WebAssembly.instantiateStreaming(module, imports);
                } catch (e) {
                    const validResponse = module.ok && EXPECTED_RESPONSE_TYPES.has(module.type);

                    if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                        console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                    } else {
                        throw e;
                    }
                }
            }

            const bytes = await module.arrayBuffer();
            return await WebAssembly.instantiate(bytes, imports);
        } else {
            const instance = await WebAssembly.instantiate(module, imports);

            if (instance instanceof WebAssembly.Instance) {
                return { instance, module };
            } else {
                return instance;
            }
        }
    }

    function __wbg_get_imports() {
        const imports = {};
        imports.wbg = {};
        imports.wbg.__wbg___wbindgen_debug_string_adfb662ae34724b6 = function(arg0, arg1) {
            const ret = debugString(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        };
        imports.wbg.__wbg___wbindgen_is_function_8d400b8b1af978cd = function(arg0) {
            const ret = typeof(arg0) === 'function';
            return ret;
        };
        imports.wbg.__wbg___wbindgen_is_object_ce774f3490692386 = function(arg0) {
            const val = arg0;
            const ret = typeof(val) === 'object' && val !== null;
            return ret;
        };
        imports.wbg.__wbg___wbindgen_is_undefined_f6b95eab589e0269 = function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        };
        imports.wbg.__wbg___wbindgen_string_get_a2a31e16edf96e42 = function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        };
        imports.wbg.__wbg___wbindgen_throw_dd24417ed36fc46e = function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        };
        imports.wbg.__wbg__wbg_cb_unref_87dfb5aaa0cbcea7 = function(arg0) {
            arg0._wbg_cb_unref();
        };
        imports.wbg.__wbg_abort_07646c894ebbf2bd = function(arg0) {
            arg0.abort();
        };
        imports.wbg.__wbg_abort_399ecbcfd6ef3c8e = function(arg0, arg1) {
            arg0.abort(arg1);
        };
        imports.wbg.__wbg_append_c5cbdf46455cc776 = function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.append(getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
        }, arguments) };
        imports.wbg.__wbg_arrayBuffer_c04af4fce566092d = function() { return handleError(function (arg0) {
            const ret = arg0.arrayBuffer();
            return ret;
        }, arguments) };
        imports.wbg.__wbg_call_abb4ff46ce38be40 = function() { return handleError(function (arg0, arg1) {
            const ret = arg0.call(arg1);
            return ret;
        }, arguments) };
        imports.wbg.__wbg_clearTimeout_15dfc3d1dcb635c6 = function() { return handleError(function (arg0, arg1) {
            arg0.clearTimeout(arg1);
        }, arguments) };
        imports.wbg.__wbg_clearTimeout_3b6a9a13d4bde075 = function(arg0) {
            const ret = clearTimeout(arg0);
            return ret;
        };
        imports.wbg.__wbg_close_0b472ca2d13f54f7 = function(arg0) {
            arg0.close();
        };
        imports.wbg.__wbg_data_8bf4ae669a78a688 = function(arg0) {
            const ret = arg0.data;
            return ret;
        };
        imports.wbg.__wbg_done_62ea16af4ce34b24 = function(arg0) {
            const ret = arg0.done;
            return ret;
        };
        imports.wbg.__wbg_error_7534b8e9a36f1ab4 = function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        };
        imports.wbg.__wbg_fetch_90447c28cc0b095e = function(arg0, arg1) {
            const ret = arg0.fetch(arg1);
            return ret;
        };
        imports.wbg.__wbg_fetch_df3fa17a5772dafb = function(arg0) {
            const ret = fetch(arg0);
            return ret;
        };
        imports.wbg.__wbg_getTime_ad1e9878a735af08 = function(arg0) {
            const ret = arg0.getTime();
            return ret;
        };
        imports.wbg.__wbg_get_af9dab7e9603ea93 = function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments) };
        imports.wbg.__wbg_has_0e670569d65d3a45 = function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.has(arg0, arg1);
            return ret;
        }, arguments) };
        imports.wbg.__wbg_headers_654c30e1bcccc552 = function(arg0) {
            const ret = arg0.headers;
            return ret;
        };
        imports.wbg.__wbg_instanceof_Response_cd74d1c2ac92cb0b = function(arg0) {
            let result;
            try {
                result = arg0 instanceof Response;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        };
        imports.wbg.__wbg_instanceof_Window_b5cf7783caa68180 = function(arg0) {
            let result;
            try {
                result = arg0 instanceof Window;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        };
        imports.wbg.__wbg_iterator_27b7c8b35ab3e86b = function() {
            const ret = Symbol.iterator;
            return ret;
        };
        imports.wbg.__wbg_length_22ac23eaec9d8053 = function(arg0) {
            const ret = arg0.length;
            return ret;
        };
        imports.wbg.__wbg_new_0_23cedd11d9b40c9d = function() {
            const ret = new Date();
            return ret;
        };
        imports.wbg.__wbg_new_1ba21ce319a06297 = function() {
            const ret = new Object();
            return ret;
        };
        imports.wbg.__wbg_new_3c79b3bb1b32b7d3 = function() { return handleError(function () {
            const ret = new Headers();
            return ret;
        }, arguments) };
        imports.wbg.__wbg_new_6421f6084cc5bc5a = function(arg0) {
            const ret = new Uint8Array(arg0);
            return ret;
        };
        imports.wbg.__wbg_new_881a222c65f168fc = function() { return handleError(function () {
            const ret = new AbortController();
            return ret;
        }, arguments) };
        imports.wbg.__wbg_new_8a6f238a6ece86ea = function() {
            const ret = new Error();
            return ret;
        };
        imports.wbg.__wbg_new_from_slice_f9c22b9153b26992 = function(arg0, arg1) {
            const ret = new Uint8Array(getArrayU8FromWasm0(arg0, arg1));
            return ret;
        };
        imports.wbg.__wbg_new_no_args_cb138f77cf6151ee = function(arg0, arg1) {
            const ret = new Function(getStringFromWasm0(arg0, arg1));
            return ret;
        };
        imports.wbg.__wbg_new_with_str_and_init_c5748f76f5108934 = function() { return handleError(function (arg0, arg1, arg2) {
            const ret = new Request(getStringFromWasm0(arg0, arg1), arg2);
            return ret;
        }, arguments) };
        imports.wbg.__wbg_next_138a17bbf04e926c = function(arg0) {
            const ret = arg0.next;
            return ret;
        };
        imports.wbg.__wbg_next_3cfe5c0fe2a4cc53 = function() { return handleError(function (arg0) {
            const ret = arg0.next();
            return ret;
        }, arguments) };
        imports.wbg.__wbg_now_2c95c9de01293173 = function(arg0) {
            const ret = arg0.now();
            return ret;
        };
        imports.wbg.__wbg_now_8cf15d6e317793e1 = function(arg0) {
            const ret = arg0.now();
            return ret;
        };
        imports.wbg.__wbg_performance_7a3ffd0b17f663ad = function(arg0) {
            const ret = arg0.performance;
            return ret;
        };
        imports.wbg.__wbg_performance_c77a440eff2efd9b = function(arg0) {
            const ret = arg0.performance;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        };
        imports.wbg.__wbg_postMessage_7243f814e0cfb266 = function() { return handleError(function (arg0, arg1) {
            arg0.postMessage(arg1);
        }, arguments) };
        imports.wbg.__wbg_prototypesetcall_dfe9b766cdc1f1fd = function(arg0, arg1, arg2) {
            Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), arg2);
        };
        imports.wbg.__wbg_queueMicrotask_9b549dfce8865860 = function(arg0) {
            const ret = arg0.queueMicrotask;
            return ret;
        };
        imports.wbg.__wbg_queueMicrotask_fca69f5bfad613a5 = function(arg0) {
            queueMicrotask(arg0);
        };
        imports.wbg.__wbg_resolve_fd5bfbaa4ce36e1e = function(arg0) {
            const ret = Promise.resolve(arg0);
            return ret;
        };
        imports.wbg.__wbg_setTimeout_35a07631c669fbee = function(arg0, arg1) {
            const ret = setTimeout(arg0, arg1);
            return ret;
        };
        imports.wbg.__wbg_setTimeout_4eb823e8b72fbe79 = function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.setTimeout(arg1, arg2);
            return ret;
        }, arguments) };
        imports.wbg.__wbg_set_body_8e743242d6076a4f = function(arg0, arg1) {
            arg0.body = arg1;
        };
        imports.wbg.__wbg_set_cache_0e437c7c8e838b9b = function(arg0, arg1) {
            arg0.cache = __wbindgen_enum_RequestCache[arg1];
        };
        imports.wbg.__wbg_set_credentials_55ae7c3c106fd5be = function(arg0, arg1) {
            arg0.credentials = __wbindgen_enum_RequestCredentials[arg1];
        };
        imports.wbg.__wbg_set_headers_5671cf088e114d2b = function(arg0, arg1) {
            arg0.headers = arg1;
        };
        imports.wbg.__wbg_set_method_76c69e41b3570627 = function(arg0, arg1, arg2) {
            arg0.method = getStringFromWasm0(arg1, arg2);
        };
        imports.wbg.__wbg_set_mode_611016a6818fc690 = function(arg0, arg1) {
            arg0.mode = __wbindgen_enum_RequestMode[arg1];
        };
        imports.wbg.__wbg_set_onmessage_5fe29d0fb54cb575 = function(arg0, arg1) {
            arg0.onmessage = arg1;
        };
        imports.wbg.__wbg_set_signal_e89be862d0091009 = function(arg0, arg1) {
            arg0.signal = arg1;
        };
        imports.wbg.__wbg_signal_3c14fbdc89694b39 = function(arg0) {
            const ret = arg0.signal;
            return ret;
        };
        imports.wbg.__wbg_stack_0ed75d68575b0f3c = function(arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        };
        imports.wbg.__wbg_static_accessor_GLOBAL_769e6b65d6557335 = function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        };
        imports.wbg.__wbg_static_accessor_GLOBAL_THIS_60cf02db4de8e1c1 = function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        };
        imports.wbg.__wbg_static_accessor_SELF_08f5a74c69739274 = function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        };
        imports.wbg.__wbg_static_accessor_WINDOW_a8924b26aa92d024 = function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        };
        imports.wbg.__wbg_status_9bfc680efca4bdfd = function(arg0) {
            const ret = arg0.status;
            return ret;
        };
        imports.wbg.__wbg_stringify_655a6390e1f5eb6b = function() { return handleError(function (arg0) {
            const ret = JSON.stringify(arg0);
            return ret;
        }, arguments) };
        imports.wbg.__wbg_text_51046bb33d257f63 = function() { return handleError(function (arg0) {
            const ret = arg0.text();
            return ret;
        }, arguments) };
        imports.wbg.__wbg_then_429f7caf1026411d = function(arg0, arg1, arg2) {
            const ret = arg0.then(arg1, arg2);
            return ret;
        };
        imports.wbg.__wbg_then_4f95312d68691235 = function(arg0, arg1) {
            const ret = arg0.then(arg1);
            return ret;
        };
        imports.wbg.__wbg_url_b6d11838a4f95198 = function(arg0, arg1) {
            const ret = arg1.url;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        };
        imports.wbg.__wbg_value_57b7b035e117f7ee = function(arg0) {
            const ret = arg0.value;
            return ret;
        };
        imports.wbg.__wbindgen_cast_067e3c0104867449 = function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { dtor_idx: 225, function: Function { arguments: [NamedExternref("MessageEvent")], shim_idx: 226, ret: Unit, inner_ret: Some(Unit) }, mutable: false }) -> Externref`.
            const ret = makeClosure(arg0, arg1, wasm.wasm_bindgen_1add006a0ed82fd3___closure__destroy___dyn_core_b125d98f3949a913___ops__function__Fn__web_sys_8b37e013ecf136cb___features__gen_MessageEvent__MessageEvent____Output_______, wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke___web_sys_8b37e013ecf136cb___features__gen_MessageEvent__MessageEvent_____);
            return ret;
        };
        imports.wbg.__wbindgen_cast_2241b6af4c4b2941 = function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        };
        imports.wbg.__wbindgen_cast_4c40eebfb345262b = function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { dtor_idx: 1169, function: Function { arguments: [], shim_idx: 1170, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm.wasm_bindgen_1add006a0ed82fd3___closure__destroy___dyn_core_b125d98f3949a913___ops__function__FnMut_____Output_______, wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke______);
            return ret;
        };
        imports.wbg.__wbindgen_cast_ba06f0048889102c = function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { dtor_idx: 2202, function: Function { arguments: [Externref], shim_idx: 2203, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm.wasm_bindgen_1add006a0ed82fd3___closure__destroy___dyn_core_b125d98f3949a913___ops__function__FnMut__wasm_bindgen_1add006a0ed82fd3___JsValue____Output_______, wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke___wasm_bindgen_1add006a0ed82fd3___JsValue_____);
            return ret;
        };
        imports.wbg.__wbindgen_cast_dd8568660229206d = function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { dtor_idx: 1434, function: Function { arguments: [], shim_idx: 1435, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm.wasm_bindgen_1add006a0ed82fd3___closure__destroy___dyn_core_b125d98f3949a913___ops__function__FnMut_____Output________1_, wasm_bindgen_1add006a0ed82fd3___convert__closures_____invoke_______1_);
            return ret;
        };
        imports.wbg.__wbindgen_init_externref_table = function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        };

        return imports;
    }

    function __wbg_finalize_init(instance, module) {
        wasm = instance.exports;
        __wbg_init.__wbindgen_wasm_module = module;
        cachedDataViewMemory0 = null;
        cachedUint8ArrayMemory0 = null;


        wasm.__wbindgen_start();
        return wasm;
    }

    function initSync(module) {
        if (wasm !== undefined) return wasm;


        if (typeof module !== 'undefined') {
            if (Object.getPrototypeOf(module) === Object.prototype) {
                ({module} = module)
            } else {
                console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
            }
        }

        const imports = __wbg_get_imports();
        if (!(module instanceof WebAssembly.Module)) {
            module = new WebAssembly.Module(module);
        }
        const instance = new WebAssembly.Instance(module, imports);
        return __wbg_finalize_init(instance, module);
    }

    async function __wbg_init(module_or_path) {
        if (wasm !== undefined) return wasm;


        if (typeof module_or_path !== 'undefined') {
            if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
                ({module_or_path} = module_or_path)
            } else {
                console.warn('using deprecated parameters for the initialization function; pass a single object instead')
            }
        }

        if (typeof module_or_path === 'undefined' && typeof script_src !== 'undefined') {
            module_or_path = script_src.replace(/\.js$/, '_bg.wasm');
        }
        const imports = __wbg_get_imports();

        if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
            module_or_path = fetch(module_or_path);
        }

        const { instance, module } = await __wbg_load(await module_or_path, imports);

        return __wbg_finalize_init(instance, module);
    }

    wasm_bindgen = Object.assign(__wbg_init, { initSync }, __exports);
})();
