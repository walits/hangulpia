/* tslint:disable */
/* eslint-disable */

/**
 * Convert Hangul (Dubeolsik) input to Japanese, with kanji substitution
 * where the vocabulary has a verified reading.
 */
export function convert(input: string): string;

/**
 * Hiragana only, no kanji — matches the real app's live-typing/composition
 * text (`hj_hangul_to_hiragana`), which never shows kanji inline.
 */
export function convert_hiragana_only(input: string): string;

/**
 * Vocabulary size the engine was built from (for a "trained on N words"
 * style footer/caption, kept truthful without hardcoding the number twice).
 */
export function vocab_size(): number;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly convert: (a: number, b: number) => [number, number];
    readonly convert_hiragana_only: (a: number, b: number) => [number, number];
    readonly vocab_size: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
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
