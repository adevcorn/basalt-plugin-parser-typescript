// basalt-plugin-parser-typescript/src/lib.rs — tree-sitter TypeScript/TSX parser WASM plugin for Basalt

use tree_sitter::{Language, Parser, Query, QueryCursor};

const SRC_OFFSET: usize = 0;
const OUT_OFFSET: usize = 6 * 1024 * 1024;
const MEMORY_BYTES: usize = 12 * 1024 * 1024;

const SCOPE_KEYWORD:   u8 = 1;
const SCOPE_STRING:    u8 = 2;
const SCOPE_NUMBER:    u8 = 3;
const SCOPE_COMMENT:   u8 = 4;
const SCOPE_TYPE:      u8 = 5;
const SCOPE_FUNCTION:  u8 = 6;
const SCOPE_OPERATOR:  u8 = 7;
const SCOPE_VARIABLE:  u8 = 10;
const SCOPE_NAMESPACE: u8 = 11;

static mut MEMORY: [u8; MEMORY_BYTES] = [0u8; MEMORY_BYTES];
static LANG_EXT:     &[u8] = b"ts\0";
static LANG_EXT_TSX: &[u8] = b"tsx\0";

extern "C" {
    fn tree_sitter_typescript() -> Language;
    fn tree_sitter_tsx() -> Language;
}

// ── query source strings ────────────────────────────────────────────────────

const PARSE_QUERY_SRC: &str = r#"
    "import" @keyword "export" @keyword "from" @keyword
    "const" @keyword "let" @keyword "var" @keyword
    "type" @keyword "interface" @keyword "class" @keyword
    "function" @keyword "return" @keyword
    "async" @keyword "await" @keyword
    "extends" @keyword "implements" @keyword
    "new" @keyword "this" @keyword
    "typeof" @keyword "keyof" @keyword
    "in" @keyword "of" @keyword "instanceof" @keyword
    "yield" @keyword "throw" @keyword
    "try" @keyword "catch" @keyword "finally" @keyword
    "break" @keyword "continue" @keyword
    "switch" @keyword "case" @keyword "default" @keyword
    "if" @keyword "else" @keyword
    "for" @keyword "while" @keyword "do" @keyword
    "delete" @keyword "void" @keyword
    "enum" @keyword "declare" @keyword "abstract" @keyword
    "readonly" @keyword "override" @keyword "static" @keyword
    "public" @keyword "private" @keyword "protected" @keyword
    "null" @keyword "undefined" @keyword "true" @keyword "false" @keyword
    (string) @string
    (template_string) @string
    (number) @number
    (comment) @comment
    (type_identifier) @type
    (predefined_type) @type
    (function_declaration name: (identifier) @function)
    (method_definition name: (property_identifier) @function)
    (variable_declarator name: (identifier) value: (arrow_function) @function)
    (variable_declarator name: (identifier) @variable)
    (formal_parameters (identifier) @variable)
    "+" @operator "-" @operator "*" @operator "/" @operator
    "%" @operator "=" @operator "+=" @operator "-=" @operator
    "==" @operator "!=" @operator "<" @operator ">" @operator
    "&&" @operator "||" @operator "!" @operator
    "?" @operator "??" @operator "=>" @operator
    "===" @operator "!==" @operator "<=" @operator ">=" @operator
"#;

const PARSE_QUERY_TSX_SRC: &str = r#"
    "import" @keyword "export" @keyword "from" @keyword
    "const" @keyword "let" @keyword "var" @keyword
    "type" @keyword "interface" @keyword "class" @keyword
    "function" @keyword "return" @keyword
    "async" @keyword "await" @keyword
    "extends" @keyword "implements" @keyword
    "new" @keyword "this" @keyword
    "typeof" @keyword "keyof" @keyword
    "in" @keyword "of" @keyword "instanceof" @keyword
    "yield" @keyword "throw" @keyword
    "try" @keyword "catch" @keyword "finally" @keyword
    "break" @keyword "continue" @keyword
    "switch" @keyword "case" @keyword "default" @keyword
    "if" @keyword "else" @keyword
    "for" @keyword "while" @keyword "do" @keyword
    "delete" @keyword "void" @keyword
    "enum" @keyword "declare" @keyword "abstract" @keyword
    "readonly" @keyword "override" @keyword "static" @keyword
    "public" @keyword "private" @keyword "protected" @keyword
    "null" @keyword "undefined" @keyword "true" @keyword "false" @keyword
    (string) @string
    (template_string) @string
    (number) @number
    (comment) @comment
    (type_identifier) @type
    (predefined_type) @type
    (function_declaration name: (identifier) @function)
    (method_definition name: (property_identifier) @function)
    (variable_declarator name: (identifier) value: (arrow_function) @function)
    (variable_declarator name: (identifier) @variable)
    (formal_parameters (identifier) @variable)
    "+" @operator "-" @operator "*" @operator "/" @operator
    "%" @operator "=" @operator "+=" @operator "-=" @operator
    "==" @operator "!=" @operator "<" @operator ">" @operator
    "&&" @operator "||" @operator "!" @operator
    "?" @operator "??" @operator "=>" @operator
    "===" @operator "!==" @operator "<=" @operator ">=" @operator
    (jsx_opening_element name: (_) @type)
    (jsx_closing_element name: (_) @type)
"#;

const RETRIEVAL_QUERY_SRC: &str = r#"
    (function_declaration name: (_) @name.function) @chunk.function
    (class_declaration name: (_) @name.type) @chunk.type
    (interface_declaration name: (_) @name.type) @chunk.type
    (type_alias_declaration name: (_) @name.type) @chunk.type
    (enum_declaration name: (_) @name.type) @chunk.type
    (lexical_declaration (variable_declarator name: (_) @name.function value: (arrow_function))) @chunk.function
    (export_statement declaration: (function_declaration name: (_) @name.function)) @chunk.function
    (module name: (_) @name.module) @chunk.module
"#;

const CALL_SITES_QUERY_SRC: &str = r#"
    (call_expression function: (identifier) @callee)
    (call_expression function: (member_expression property: (property_identifier) @callee))
"#;

// ── parser state ────────────────────────────────────────────────────────────

struct ParserState {
    parser: Parser,
    parse_query: Query,
    retrieval_query: Query,
    call_sites_query: Query,
    parse_cap_names: Vec<String>,
    retrieval_cap_names: Vec<String>,
}

struct TsxParserState {
    parser: Parser,
    parse_query: Query,
    retrieval_query: Query,
    call_sites_query: Query,
    parse_cap_names: Vec<String>,
    retrieval_cap_names: Vec<String>,
}

static mut STATE:     Option<ParserState>    = None;
static mut STATE_TSX: Option<TsxParserState> = None;

unsafe fn get_state() -> Option<&'static mut ParserState> {
    if STATE.is_none() {
        let lang = tree_sitter_typescript();
        let mut parser = Parser::new();
        parser.set_language(lang).ok()?;

        let parse_query = Query::new(lang, PARSE_QUERY_SRC).ok()?;
        let parse_cap_names: Vec<String> =
            parse_query.capture_names().iter().map(|s| s.to_string()).collect();

        let retrieval_query = Query::new(lang, RETRIEVAL_QUERY_SRC).ok()?;
        let retrieval_cap_names: Vec<String> =
            retrieval_query.capture_names().iter().map(|s| s.to_string()).collect();

        let call_sites_query = Query::new(lang, CALL_SITES_QUERY_SRC).ok()?;

        STATE = Some(ParserState {
            parser,
            parse_query,
            retrieval_query,
            call_sites_query,
            parse_cap_names,
            retrieval_cap_names,
        });
    }
    STATE.as_mut()
}

unsafe fn get_state_tsx() -> Option<&'static mut TsxParserState> {
    if STATE_TSX.is_none() {
        let lang = tree_sitter_tsx();
        let mut parser = Parser::new();
        parser.set_language(lang).ok()?;

        let parse_query = Query::new(lang, PARSE_QUERY_TSX_SRC).ok()?;
        let parse_cap_names: Vec<String> =
            parse_query.capture_names().iter().map(|s| s.to_string()).collect();

        let retrieval_query = Query::new(lang, RETRIEVAL_QUERY_SRC).ok()?;
        let retrieval_cap_names: Vec<String> =
            retrieval_query.capture_names().iter().map(|s| s.to_string()).collect();

        let call_sites_query = Query::new(lang, CALL_SITES_QUERY_SRC).ok()?;

        STATE_TSX = Some(TsxParserState {
            parser,
            parse_query,
            retrieval_query,
            call_sites_query,
            parse_cap_names,
            retrieval_cap_names,
        });
    }
    STATE_TSX.as_mut()
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn scope_id_for(name: &str) -> u8 {
    match name {
        "keyword"   => SCOPE_KEYWORD,
        "string"    => SCOPE_STRING,
        "number"    => SCOPE_NUMBER,
        "comment"   => SCOPE_COMMENT,
        "type"      => SCOPE_TYPE,
        "function"  => SCOPE_FUNCTION,
        "operator"  => SCOPE_OPERATOR,
        "variable"  => SCOPE_VARIABLE,
        "namespace" => SCOPE_NAMESPACE,
        _           => 0,
    }
}

fn kind_byte(k: &str) -> u8 {
    match k {
        "module"   => 1,
        "type"     => 2,
        "function" => 3,
        _          => 0,
    }
}

// ── exports ──────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn basalt_lang() -> i32 {
    LANG_EXT.as_ptr() as i32
}

#[no_mangle]
pub extern "C" fn basalt_lang_tsx() -> i32 {
    LANG_EXT_TSX.as_ptr() as i32
}

#[no_mangle]
pub unsafe extern "C" fn basalt_src_ptr() -> i32 {
    MEMORY[SRC_OFFSET..].as_ptr() as i32
}

#[no_mangle]
pub unsafe extern "C" fn basalt_out_ptr() -> i32 {
    MEMORY[OUT_OFFSET..].as_ptr() as i32
}

// ── parse (TS) ───────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn basalt_parse(
    src_ptr: i32, src_len: i32, out_ptr: i32, max_spans: i32,
) -> i32 {
    let src = std::slice::from_raw_parts(src_ptr as usize as *const u8, src_len as usize);
    let out = std::slice::from_raw_parts_mut(out_ptr as usize as *mut u8, (max_spans as usize) * 12);
    let state = match get_state() { Some(s) => s, None => return 0 };
    state.parser.reset();
    let Some(tree) = state.parser.parse(src, None) else { return 0 };
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&state.parse_query, tree.root_node(), src);
    let mut count = 0usize;
    for m in matches {
        for cap in m.captures {
            if count >= max_spans as usize { break; }
            let scope_id = scope_id_for(&state.parse_cap_names[cap.index as usize]);
            let offset = cap.node.start_byte() as u32;
            let length = (cap.node.end_byte() - cap.node.start_byte()) as u32;
            let base = count * 12;
            out[base..base+4].copy_from_slice(&offset.to_le_bytes());
            out[base+4..base+8].copy_from_slice(&length.to_le_bytes());
            out[base+8] = scope_id;
            out[base+9] = 0; out[base+10] = 0; out[base+11] = 0;
            count += 1;
        }
    }
    count as i32
}

// ── parse (TSX) ──────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn basalt_parse_tsx(
    src_ptr: i32, src_len: i32, out_ptr: i32, max_spans: i32,
) -> i32 {
    let src = std::slice::from_raw_parts(src_ptr as usize as *const u8, src_len as usize);
    let out = std::slice::from_raw_parts_mut(out_ptr as usize as *mut u8, (max_spans as usize) * 12);
    let state = match get_state_tsx() { Some(s) => s, None => return 0 };
    state.parser.reset();
    let Some(tree) = state.parser.parse(src, None) else { return 0 };
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&state.parse_query, tree.root_node(), src);
    let mut count = 0usize;
    for m in matches {
        for cap in m.captures {
            if count >= max_spans as usize { break; }
            let scope_id = scope_id_for(&state.parse_cap_names[cap.index as usize]);
            let offset = cap.node.start_byte() as u32;
            let length = (cap.node.end_byte() - cap.node.start_byte()) as u32;
            let base = count * 12;
            out[base..base+4].copy_from_slice(&offset.to_le_bytes());
            out[base+4..base+8].copy_from_slice(&length.to_le_bytes());
            out[base+8] = scope_id;
            out[base+9] = 0; out[base+10] = 0; out[base+11] = 0;
            count += 1;
        }
    }
    count as i32
}

// ── retrieval chunks (TS) ────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn basalt_retrieval_chunks(
    src_ptr: i32, src_len: i32, out_ptr: i32, max_chunks: i32,
) -> i32 {
    let src = std::slice::from_raw_parts(src_ptr as usize as *const u8, src_len as usize);
    let out = std::slice::from_raw_parts_mut(out_ptr as usize as *mut u8, (max_chunks as usize) * 104);
    let state = match get_state() { Some(s) => s, None => return 0 };
    state.parser.reset();
    let Some(tree) = state.parser.parse(src, None) else { return 0 };
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&state.retrieval_query, tree.root_node(), src);
    let mut count = 0usize;
    for m in matches {
        if count >= max_chunks as usize { break; }
        let mut offset = None::<u32>;
        let mut length = None::<u32>;
        let mut kind   = None::<&str>;
        let mut name   = None::<&str>;
        for cap in m.captures {
            let cn = &state.retrieval_cap_names[cap.index as usize];
            if let Some(k) = cn.strip_prefix("chunk.") {
                offset = Some(cap.node.start_byte() as u32);
                length = Some((cap.node.end_byte() - cap.node.start_byte()) as u32);
                kind   = Some(k);
            } else if cn.starts_with("name.") {
                if let Ok(t) = cap.node.utf8_text(src) { name = Some(t.trim()); }
            }
        }
        let (Some(off), Some(len), Some(k)) = (offset, length, kind) else { continue };
        let label = if let Some(n) = name {
            let mut s = k.to_string(); s.push(' '); s.push_str(n); s
        } else { k.to_string() };
        let base = count * 104;
        out[base..base+4].copy_from_slice(&off.to_le_bytes());
        out[base+4..base+8].copy_from_slice(&len.to_le_bytes());
        let lbytes = label.as_bytes();
        let llen = lbytes.len().min(95);
        out[base+8..base+8+llen].copy_from_slice(&lbytes[..llen]);
        out[base+8+llen] = 0;
        out[base+103] = kind_byte(k);
        count += 1;
    }
    count as i32
}

// ── retrieval chunks (TSX) ───────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn basalt_retrieval_chunks_tsx(
    src_ptr: i32, src_len: i32, out_ptr: i32, max_chunks: i32,
) -> i32 {
    let src = std::slice::from_raw_parts(src_ptr as usize as *const u8, src_len as usize);
    let out = std::slice::from_raw_parts_mut(out_ptr as usize as *mut u8, (max_chunks as usize) * 104);
    let state = match get_state_tsx() { Some(s) => s, None => return 0 };
    state.parser.reset();
    let Some(tree) = state.parser.parse(src, None) else { return 0 };
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&state.retrieval_query, tree.root_node(), src);
    let mut count = 0usize;
    for m in matches {
        if count >= max_chunks as usize { break; }
        let mut offset = None::<u32>;
        let mut length = None::<u32>;
        let mut kind   = None::<&str>;
        let mut name   = None::<&str>;
        for cap in m.captures {
            let cn = &state.retrieval_cap_names[cap.index as usize];
            if let Some(k) = cn.strip_prefix("chunk.") {
                offset = Some(cap.node.start_byte() as u32);
                length = Some((cap.node.end_byte() - cap.node.start_byte()) as u32);
                kind   = Some(k);
            } else if cn.starts_with("name.") {
                if let Ok(t) = cap.node.utf8_text(src) { name = Some(t.trim()); }
            }
        }
        let (Some(off), Some(len), Some(k)) = (offset, length, kind) else { continue };
        let label = if let Some(n) = name {
            let mut s = k.to_string(); s.push(' '); s.push_str(n); s
        } else { k.to_string() };
        let base = count * 104;
        out[base..base+4].copy_from_slice(&off.to_le_bytes());
        out[base+4..base+8].copy_from_slice(&len.to_le_bytes());
        let lbytes = label.as_bytes();
        let llen = lbytes.len().min(95);
        out[base+8..base+8+llen].copy_from_slice(&lbytes[..llen]);
        out[base+8+llen] = 0;
        out[base+103] = kind_byte(k);
        count += 1;
    }
    count as i32
}

// ── call sites ───────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn basalt_call_sites(
    src_ptr: i32, src_len: i32, out_ptr: i32, max_sites: i32,
) -> i32 {
    let src = std::slice::from_raw_parts(src_ptr as usize as *const u8, src_len as usize);
    let out = std::slice::from_raw_parts_mut(out_ptr as usize as *mut u8, (max_sites as usize) * 68);
    let state = match get_state() { Some(s) => s, None => return 0 };
    state.parser.reset();
    let Some(tree) = state.parser.parse(src, None) else { return 0 };
    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&state.call_sites_query, tree.root_node(), src);
    let mut count = 0usize;
    for m in matches {
        if count >= max_sites as usize { break; }
        for cap in m.captures {
            let Ok(name) = cap.node.utf8_text(src) else { continue };
            let name = name.trim();
            if name.is_empty() { continue; }
            let offset = cap.node.start_byte() as u32;
            let base = count * 68;
            out[base..base+4].copy_from_slice(&offset.to_le_bytes());
            let nb = name.as_bytes();
            let nlen = nb.len().min(63);
            out[base+4..base+4+nlen].copy_from_slice(&nb[..nlen]);
            out[base+4+nlen] = 0;
            count += 1;
        }
    }
    count as i32
}