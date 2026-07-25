/* @ts-self-types="./samkhya_wasm.d.ts" */

/**
 * Bloom filter sized for `n_items` at a target false-positive rate.
 */
export class BloomFilter {
    static __wrap(ptr) {
        const obj = Object.create(BloomFilter.prototype);
        obj.__wbg_ptr = ptr;
        BloomFilterFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BloomFilterFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bloomfilter_free(ptr, 0);
    }
    /**
     * Insert a value.
     * @param {string} item
     */
    add(item) {
        const ptr0 = passStringToWasm0(item, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bloomfilter_add(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * `true` if the filter may contain the value, `false` if it definitely
     * does not. False positives are possible; false negatives are not.
     * @param {string} item
     * @returns {boolean}
     */
    contains(item) {
        const ptr0 = passStringToWasm0(item, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bloomfilter_contains(this.__wbg_ptr, ptr0, len0);
        return ret !== 0;
    }
    /**
     * Restore from [`Self::to_bytes`].
     * @param {Uint8Array} data
     * @returns {BloomFilter}
     */
    static fromBytes(data) {
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bloomfilter_fromBytes(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return BloomFilter.__wrap(ret[0]);
    }
    /**
     * Build a filter. `fp_rate` must be in `(0, 1)`.
     * @param {number} n_items
     * @param {number} fp_rate
     */
    constructor(n_items, fp_rate) {
        const ret = wasm.bloomfilter_new(n_items, fp_rate);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        BloomFilterFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Serialise to the portable payload.
     * @returns {Uint8Array}
     */
    toBytes() {
        const ret = wasm.bloomfilter_toBytes(this.__wbg_ptr);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
}
if (Symbol.dispose) BloomFilter.prototype[Symbol.dispose] = BloomFilter.prototype.free;

/**
 * Count-Min frequency sketch.
 */
export class CountMinSketch {
    static __wrap(ptr) {
        const obj = Object.create(CountMinSketch.prototype);
        obj.__wbg_ptr = ptr;
        CountMinSketchFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        CountMinSketchFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_countminsketch_free(ptr, 0);
    }
    /**
     * Add `count` occurrences of a value.
     * @param {string} item
     * @param {number} count
     */
    add(item, count) {
        const ptr0 = passStringToWasm0(item, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.countminsketch_add(this.__wbg_ptr, ptr0, len0, count);
    }
    /**
     * Frequency estimate. Never below the truth unless the sketch has
     * saturated — check [`Self::is_saturated`].
     * @param {string} item
     * @returns {number}
     */
    estimate(item) {
        const ptr0 = passStringToWasm0(item, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.countminsketch_estimate(this.__wbg_ptr, ptr0, len0);
        return ret;
    }
    /**
     * Restore from [`Self::to_bytes`].
     * @param {Uint8Array} data
     * @returns {CountMinSketch}
     */
    static fromBytes(data) {
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.countminsketch_fromBytes(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return CountMinSketch.__wrap(ret[0]);
    }
    /**
     * Whether any counter has reached its maximum, which is the one
     * condition under which the never-undercount guarantee fails.
     * @returns {boolean}
     */
    isSaturated() {
        const ret = wasm.countminsketch_isSaturated(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * An upper bound on the frequency of the most frequent value, without
     * needing to know which value that is. `null` when saturated.
     * @returns {number | undefined}
     */
    maxFrequencyBound() {
        const ret = wasm.countminsketch_maxFrequencyBound(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : ret[1];
    }
    /**
     * Build a sketch with `depth` hash rows of `width` counters each.
     * @param {number} depth
     * @param {number} width
     */
    constructor(depth, width) {
        const ret = wasm.countminsketch_new(depth, width);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        CountMinSketchFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Serialise to the portable payload.
     * @returns {Uint8Array}
     */
    toBytes() {
        const ret = wasm.countminsketch_toBytes(this.__wbg_ptr);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
}
if (Symbol.dispose) CountMinSketch.prototype[Symbol.dispose] = CountMinSketch.prototype.free;

/**
 * HyperLogLog distinct-count sketch.
 *
 * Precision `p` selects `2^p` registers; relative error is about
 * `1.04 / sqrt(2^p)`. Valid range is 4 to 18.
 */
export class HllSketch {
    static __wrap(ptr) {
        const obj = Object.create(HllSketch.prototype);
        obj.__wbg_ptr = ptr;
        HllSketchFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        HllSketchFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_hllsketch_free(ptr, 0);
    }
    /**
     * Add a value. Strings are hashed as UTF-8 bytes.
     * @param {string} item
     */
    add(item) {
        const ptr0 = passStringToWasm0(item, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.hllsketch_add(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Add raw bytes, for callers that already have a stable encoding.
     * @param {Uint8Array} item
     */
    addBytes(item) {
        const ptr0 = passArray8ToWasm0(item, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.hllsketch_addBytes(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * A distinct count that is never above the truth.
     *
     * Every value hashes to exactly one register, so a register is non-zero
     * only if some distinct value reached it. Collisions only push the count
     * further down. Weak, but sound — which is what a provable bound needs.
     * @returns {number}
     */
    distinctFloor() {
        const ret = wasm.hllsketch_distinctFloor(this.__wbg_ptr);
        return ret;
    }
    /**
     * Current distinct-count estimate.
     *
     * This is a two-sided estimate: it lands above the truth about half the
     * time. Use [`Self::distinct_floor`] where a value that is never above
     * the truth is required — deriving a join ceiling, for instance.
     * @returns {number}
     */
    estimate() {
        const ret = wasm.hllsketch_estimate(this.__wbg_ptr);
        return ret;
    }
    /**
     * Restore a sketch from [`Self::to_bytes`], validating it first.
     * @param {Uint8Array} data
     * @returns {HllSketch}
     */
    static fromBytes(data) {
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.hllsketch_fromBytes(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return HllSketch.__wrap(ret[0]);
    }
    /**
     * Merge another sketch of the same precision into this one.
     * @param {HllSketch} other
     */
    merge(other) {
        _assertClass(other, HllSketch);
        const ret = wasm.hllsketch_merge(this.__wbg_ptr, other.__wbg_ptr);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Build an empty sketch at precision `p`.
     * @param {number} p
     */
    constructor(p) {
        const ret = wasm.hllsketch_new(p);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        HllSketchFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Precision the sketch was built at.
     * @returns {number}
     */
    get precision() {
        const ret = wasm.hllsketch_precision(this.__wbg_ptr);
        return ret;
    }
    /**
     * Serialise to the portable payload every samkhya binding reads.
     * @returns {Uint8Array}
     */
    toBytes() {
        const ret = wasm.hllsketch_toBytes(this.__wbg_ptr);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
}
if (Symbol.dispose) HllSketch.prototype[Symbol.dispose] = HllSketch.prototype.free;

/**
 * A number the join provably cannot exceed.
 *
 * `rows[i]` is the row count of relation `i`. `edges` is a flat list of
 * relation-index pairs — `[0, 1, 1, 2]` means relation 0 joins relation 1,
 * and relation 1 joins relation 2. `distinct_counts[i]`, when supplied, is
 * the number of distinct join-key values in relation `i`.
 *
 * # The distinct counts must not be over-stated
 *
 * The bound derives a maximum degree as `rows - distinct + 1`, so it
 * subtracts the distinct count. A value above the truth produces a ceiling
 * *below* the truth, which defeats the point. Pass
 * [`HllSketch::distinct_floor`], not [`HllSketch::estimate`].
 *
 * Pass an empty array to omit them entirely: the result is then the
 * Cartesian product, which is sound and useless — the honest answer when
 * nothing better is known.
 *
 * # Example
 *
 * ```js
 * // 10 orders, 100 line items, 10 distinct order keys on both sides.
 * joinCeiling([10, 100], [0, 1], [10, 10]);   // 100 — exactly the truth
 * joinCeiling([10, 100], [0, 1], []);         // 1000 — the product
 * ```
 * @param {Float64Array} rows
 * @param {Uint32Array} edges
 * @param {Float64Array} distinct_counts
 * @returns {number}
 */
export function joinCeiling(rows, edges, distinct_counts) {
    const ptr0 = passArrayF64ToWasm0(rows, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passArray32ToWasm0(edges, wasm.__wbindgen_malloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passArrayF64ToWasm0(distinct_counts, wasm.__wbindgen_malloc);
    const len2 = WASM_VECTOR_LEN;
    const ret = wasm.joinCeiling(ptr0, len0, ptr1, len1, ptr2, len2);
    return ret;
}

/**
 * The Cartesian product of the row counts — the ceiling that holds when
 * nothing is known about the join keys.
 * @param {Float64Array} rows
 * @returns {number}
 */
export function productBound(rows) {
    const ptr0 = passArrayF64ToWasm0(rows, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.productBound(ptr0, len0);
    return ret;
}

/**
 * Version of the underlying samkhya crate.
 * @returns {string}
 */
export function version() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.version();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_bce6d499ff0a4aff: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg___wbindgen_throw_9c31b086c2b26051: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./samkhya_wasm_bg.js": import0,
    };
}

const BloomFilterFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bloomfilter_free(ptr, 1));
const CountMinSketchFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_countminsketch_free(ptr, 1));
const HllSketchFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_hllsketch_free(ptr, 1));

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedFloat64ArrayMemory0 = null;
function getFloat64ArrayMemory0() {
    if (cachedFloat64ArrayMemory0 === null || cachedFloat64ArrayMemory0.byteLength === 0) {
        cachedFloat64ArrayMemory0 = new Float64Array(wasm.memory.buffer);
    }
    return cachedFloat64ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function passArray32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getUint32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF64ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 8, 8) >>> 0;
    getFloat64ArrayMemory0().set(arg, ptr / 8);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
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

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
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
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedFloat64ArrayMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
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

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
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


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('samkhya_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
