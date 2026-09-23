# basalt-plugin-parser-typescript

Tree-sitter TypeScript/TSX parser WASM plugin for Basalt.

## File extensions
- `.ts`, `.mts`, `.cts` — TypeScript
- `.tsx` — TSX (React)

## Exports (WASM ABI)
- `basalt_lang` — returns pointer to `"ts\0"`
- `basalt_lang_tsx` — returns pointer to `"tsx\0"`
- `basalt_src_ptr` / `basalt_out_ptr` — shared memory buffer pointers
- `basalt_parse(src_ptr, src_len, out_ptr, max_spans) -> i32` — syntax highlight spans
- `basalt_parse_tsx(src_ptr, src_len, out_ptr, max_spans) -> i32` — TSX variant
- `basalt_retrieval_chunks(src_ptr, src_len, out_ptr, max_chunks) -> i32`
- `basalt_call_sites(src_ptr, src_len, out_ptr, max_sites) -> i32`