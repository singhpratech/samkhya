/* tslint:disable */
/* eslint-disable */

/**
 * Bloom filter sized for `n_items` at a target false-positive rate.
 */
export class BloomFilter {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Insert a value.
     */
    add(item: string): void;
    /**
     * `true` if the filter may contain the value, `false` if it definitely
     * does not. False positives are possible; false negatives are not.
     */
    contains(item: string): boolean;
    /**
     * Restore from [`Self::to_bytes`].
     */
    static fromBytes(data: Uint8Array): BloomFilter;
    /**
     * Build a filter. `fp_rate` must be in `(0, 1)`.
     */
    constructor(n_items: number, fp_rate: number);
    /**
     * Serialise to the portable payload.
     */
    toBytes(): Uint8Array;
}

/**
 * Count-Min frequency sketch.
 */
export class CountMinSketch {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Add `count` occurrences of a value.
     */
    add(item: string, count: number): void;
    /**
     * Frequency estimate. Never below the truth unless the sketch has
     * saturated — check [`Self::is_saturated`].
     */
    estimate(item: string): number;
    /**
     * Restore from [`Self::to_bytes`].
     */
    static fromBytes(data: Uint8Array): CountMinSketch;
    /**
     * Whether any counter has reached its maximum, which is the one
     * condition under which the never-undercount guarantee fails.
     */
    isSaturated(): boolean;
    /**
     * An upper bound on the frequency of the most frequent value, without
     * needing to know which value that is. `null` when saturated.
     */
    maxFrequencyBound(): number | undefined;
    /**
     * Build a sketch with `depth` hash rows of `width` counters each.
     */
    constructor(depth: number, width: number);
    /**
     * Serialise to the portable payload.
     */
    toBytes(): Uint8Array;
}

/**
 * HyperLogLog distinct-count sketch.
 *
 * Precision `p` selects `2^p` registers; relative error is about
 * `1.04 / sqrt(2^p)`. Valid range is 4 to 18.
 */
export class HllSketch {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Add a value. Strings are hashed as UTF-8 bytes.
     */
    add(item: string): void;
    /**
     * Add raw bytes, for callers that already have a stable encoding.
     */
    addBytes(item: Uint8Array): void;
    /**
     * A distinct count that is never above the truth.
     *
     * Every value hashes to exactly one register, so a register is non-zero
     * only if some distinct value reached it. Collisions only push the count
     * further down. Weak, but sound — which is what a provable bound needs.
     */
    distinctFloor(): number;
    /**
     * Current distinct-count estimate.
     *
     * This is a two-sided estimate: it lands above the truth about half the
     * time. Use [`Self::distinct_floor`] where a value that is never above
     * the truth is required — deriving a join ceiling, for instance.
     */
    estimate(): number;
    /**
     * Restore a sketch from [`Self::to_bytes`], validating it first.
     */
    static fromBytes(data: Uint8Array): HllSketch;
    /**
     * Merge another sketch of the same precision into this one.
     */
    merge(other: HllSketch): void;
    /**
     * Build an empty sketch at precision `p`.
     */
    constructor(p: number);
    /**
     * Serialise to the portable payload every samkhya binding reads.
     */
    toBytes(): Uint8Array;
    /**
     * Precision the sketch was built at.
     */
    readonly precision: number;
}

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
 */
export function joinCeiling(rows: Float64Array, edges: Uint32Array, distinct_counts: Float64Array): number;

/**
 * The Cartesian product of the row counts — the ceiling that holds when
 * nothing is known about the join keys.
 */
export function productBound(rows: Float64Array): number;

/**
 * Version of the underlying samkhya crate.
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_bloomfilter_free: (a: number, b: number) => void;
    readonly __wbg_countminsketch_free: (a: number, b: number) => void;
    readonly __wbg_hllsketch_free: (a: number, b: number) => void;
    readonly bloomfilter_add: (a: number, b: number, c: number) => void;
    readonly bloomfilter_contains: (a: number, b: number, c: number) => number;
    readonly bloomfilter_fromBytes: (a: number, b: number) => [number, number, number];
    readonly bloomfilter_new: (a: number, b: number) => [number, number, number];
    readonly bloomfilter_toBytes: (a: number) => [number, number, number, number];
    readonly countminsketch_add: (a: number, b: number, c: number, d: number) => void;
    readonly countminsketch_estimate: (a: number, b: number, c: number) => number;
    readonly countminsketch_fromBytes: (a: number, b: number) => [number, number, number];
    readonly countminsketch_isSaturated: (a: number) => number;
    readonly countminsketch_maxFrequencyBound: (a: number) => [number, number];
    readonly countminsketch_new: (a: number, b: number) => [number, number, number];
    readonly countminsketch_toBytes: (a: number) => [number, number, number, number];
    readonly hllsketch_add: (a: number, b: number, c: number) => void;
    readonly hllsketch_addBytes: (a: number, b: number, c: number) => void;
    readonly hllsketch_distinctFloor: (a: number) => number;
    readonly hllsketch_estimate: (a: number) => number;
    readonly hllsketch_fromBytes: (a: number, b: number) => [number, number, number];
    readonly hllsketch_merge: (a: number, b: number) => [number, number];
    readonly hllsketch_new: (a: number) => [number, number, number];
    readonly hllsketch_precision: (a: number) => number;
    readonly hllsketch_toBytes: (a: number) => [number, number, number, number];
    readonly joinCeiling: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
    readonly productBound: (a: number, b: number) => number;
    readonly version: () => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
