//! Build script to auto-generate Lua type definitions for LSP support.
//!
//! This script parses `src/lua/api.rs` and generates `lua/pokecon.d.lua`
//! containing EmmyLua-style type annotations for the `pokecon` global table.
//!
//! Run automatically during `cargo build`; force regeneration with `cargo clean`.
//!
//! # Output location
//!
//! The generated file is written to `OUT_DIR/pokecon.d.lua` (Cargo's build
//! output directory).  To use it in your editor, copy or symlink it into your
//! Lua workspace:
//!
//! ```bash
//! cp target/debug/build/pokecon-core-*/out/pokecon.d.lua lua/
//! ```

use std::fs;
use std::path::PathBuf;

/// Lua type definition template
const LUA_TYPE_HEADER: &str = r#"---@meta
--- Auto-generated Lua type definitions for PokeCon API.
--- Regenerate by running `cargo build` in `rust/pokecon-core/`.
---
--- Usage: copy `target/debug/build/pokecon-core-*/out/pokecon.d.lua`
--- to your Lua workspace for LSP completion.

"#;

fn main() {
    println!("cargo:rerun-if-changed=src/lua/api.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let api_rs_path = manifest_dir.join("src/lua/api.rs");

    // Parse api.rs and generate type definitions
    let lua_types = if api_rs_path.exists() {
        let api_source = fs::read_to_string(&api_rs_path).expect("Failed to read src/lua/api.rs");
        generate_lua_types_from_source(&api_source)
    } else {
        // Fallback to static definitions if api.rs not found
        generate_lua_types_static()
    };

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let out_path = out_dir.join("pokecon.d.lua");
    fs::write(&out_path, &lua_types).expect("Failed to write pokecon.d.lua");

    eprintln!("Generated Lua types: {}", out_path.display());
}

/// Parse api.rs source code and extract function signatures for Lua type generation.
///
/// This function looks for patterns like:
/// ```ignore
/// api.set(
///     "function_name",
///     lua.create_function(|_, (arg1, arg2): (Type1, Type2)| { ... })?,
/// )?;
/// ```
///
/// And generates corresponding EmmyLua type annotations with parameter names.
fn generate_lua_types_from_source(api_source: &str) -> String {
    let mut output = String::from(LUA_TYPE_HEADER);

    // Extract all api.set("name", ...) patterns
    let mut functions: Vec<LuaFunctionDef> = Vec::new();

    let lines: Vec<&str> = api_source.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Look for api.set(
        if line.starts_with("api.set(") || line.contains("api.set(") {
            // Extract function name from the next line or same line
            let name = extract_string_literal(&lines, &mut i);

            // Look for lua.create_function pattern
            while i < lines.len() && !lines[i].contains("create_function") {
                i += 1;
            }

            if i < lines.len() {
                // Extract function signature
                if let Some(func_def) = extract_function_signature(&lines, &mut i, &name) {
                    functions.push(func_def);
                }
            }
        }

        i += 1;
    }

    // If we couldn't parse any functions, fall back to static definitions
    if functions.is_empty() {
        println!(
            "cargo:warning=Lua type generation: failed to parse any functions from api.rs, using static fallback"
        );
        return generate_lua_types_static();
    }

    // Validate: warn if we parsed fewer functions than expected
    const EXPECTED_FUNCTION_COUNT: usize = 18;
    if functions.len() < EXPECTED_FUNCTION_COUNT {
        println!(
            "cargo:warning=Lua type generation: parsed {} functions, expected {}. Some functions may be missing from type definitions.",
            functions.len(),
            EXPECTED_FUNCTION_COUNT
        );
    }

    // Generate class definition
    output.push_str("---@class PokeConApi\n");
    for func in &functions {
        output.push_str(&format!("---@field {} {}\n", func.name, func.signature));
    }
    output.push('\n');

    // Generate global variable
    output.push_str("---@type PokeConApi\n");
    output.push_str("pokecon = {}\n");
    output.push('\n');

    // Generate helper types
    output.push_str("---@class PokeConPressOpts\n");
    output.push_str(
        "---@field duration? integer @Button press duration in milliseconds (default: 100)\n",
    );
    output.push_str(
        "---@field wait? integer @Wait time after press in milliseconds (default: 100)\n",
    );
    output.push('\n');

    output.push_str("---@class PokeConHoldOpts\n");
    output.push_str("---@field wait? integer @Hold duration in milliseconds (default: 100)\n");
    output.push('\n');

    output.push_str("---@class PokeConAutocmdOpts\n");
    output.push_str("---@field callback? string @Callback function reference\n");
    output.push_str("---@field group? string @Autocmd group name\n");
    output.push('\n');

    output.push_str("---@alias PokeConEventName string | \"CommandStartPre\" | \"CommandStartPost\" | \"CommandEndPre\" | \"CommandEndPost\" | \"EncounterShiny\" | \"Error\"\n");
    output.push('\n');

    output.push_str("---@class PokeConEvent\n");
    output.push_str("---@field name string @Event name\n");
    output.push_str("---@field data? table @Event payload\n");
    output.push_str("---@field propagation_stopped boolean @Whether propagation was stopped\n");
    output.push('\n');

    output
}

/// Extract a string literal from lines starting at position i.
///
/// Looks for the first pair of double quotes on the current or subsequent lines.
fn extract_string_literal(lines: &[&str], i: &mut usize) -> String {
    while *i < lines.len() {
        let line = lines[*i];
        // Look for "name" pattern
        if let Some(start) = line.find('"') {
            if let Some(end) = line[start + 1..].find('"') {
                return line[start + 1..start + 1 + end].to_string();
            }
        }
        *i += 1;
    }
    String::new()
}

/// Extract function signature from create_function pattern.
///
/// Parses closure parameters like:
/// - `|_, (arg1, arg2): (Type1, Type2)|` → `fun(arg1: Type1, arg2: Type2)`
/// - `|_, arg: Type|` → `fun(arg: Type)`
/// - `move |lua, (arg1, arg2): (Type1, Type2)|` → same as above
fn extract_function_signature(lines: &[&str], i: &mut usize, name: &str) -> Option<LuaFunctionDef> {
    let line = lines[*i].trim();

    // Look for closure parameters:
    // - |_, (args): (types)| — regular closure, ignore first param
    // - |_, arg: Type| — regular closure with single param
    // - move |lua, (args): (types)| — move closure with lua state
    // - move |_, arg: Type| — move closure, ignore first param
    let (params_start, skip_len) = if let Some(pos) = line.find("|_, ") {
        (pos, 4) // "|_, " is 4 chars
    } else if let Some(pos) = line.find("|lua, ") {
        (pos, 6) // "|lua, " is 6 chars
    } else {
        return None;
    };
    let params_str = &line[params_start + skip_len..];

    // Check for tuple pattern: (arg1, arg2): (Type1, Type2)
    if params_str.starts_with('(') {
        // Handle both "| {" and "|{" patterns
        let types_end = params_str.find("| {").or_else(|| params_str.find("|{"))?;
        let types_section = &params_str[..types_end];

        // Parse names and types from (arg1, arg2): (Type1, Type2)
        if let Some(colon_pos) = types_section.find(": ") {
            let names_str = &types_section[..colon_pos];
            let types_str = &types_section[colon_pos + 2..];

            let lua_params = parse_rust_params_to_lua(names_str, types_str);
            let signature = format!("fun({}) @{}", lua_params.join(", "), name);

            return Some(LuaFunctionDef {
                name: name.to_string(),
                signature,
            });
        }
    } else {
        // Single parameter pattern: arg: Type
        if let Some(colon_pos) = params_str.find(": ") {
            let name_str = &params_str[..colon_pos];
            let type_str = &params_str[colon_pos + 2..];
            if let Some(end_pos) = type_str.find(['|', ',']) {
                let rust_type = &type_str[..end_pos].trim();
                let lua_type = rust_type_to_lua(rust_type);
                let param_name = sanitize_param_name(name_str.trim());
                let signature = format!("fun({}: {}) @{}", param_name, lua_type, name);

                return Some(LuaFunctionDef {
                    name: name.to_string(),
                    signature,
                });
            }
        }
    }

    None
}

/// Parse Rust parameter names and types into Lua/EmmyLua parameter annotations.
///
/// Input: names = "(event_name, callback)", types = "(String, LuaFunction)"
/// Output: ["event_name: string", "callback: fun(...)"]
fn parse_rust_params_to_lua(names_str: &str, types_str: &str) -> Vec<String> {
    let names = parse_tuple_elements(names_str);
    let types = parse_rust_types_to_lua(types_str);

    names
        .into_iter()
        .zip(types)
        .map(|(name, ty)| {
            let param_name = sanitize_param_name(&name);
            format!("{}: {}", param_name, ty)
        })
        .collect()
}

/// Extract elements from a tuple string like "(a, b, c)" → ["a", "b", "c"]
fn parse_tuple_elements(s: &str) -> Vec<String> {
    let trimmed = s.trim();
    let inner = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    let mut result = Vec::new();
    let mut depth = 0;
    let mut current = String::new();

    for c in inner.chars() {
        match c {
            '<' | '(' => {
                depth += 1;
                current.push(c);
            }
            '>' | ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                result.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(c),
        }
    }

    if !current.is_empty() {
        result.push(current.trim().to_string());
    }

    result
}

/// Sanitize a Rust parameter name for Lua usage.
/// Removes leading underscore (Rust unused marker).
fn sanitize_param_name(name: &str) -> String {
    let trimmed = name.trim();
    if let Some(stripped) = trimmed.strip_prefix('_') {
        stripped.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Parse Rust type string(s) and convert to Lua type annotations.
fn parse_rust_types_to_lua(types_str: &str) -> Vec<String> {
    let mut result = Vec::new();
    let trimmed = types_str.trim();

    // Handle Option<Type> pattern
    if trimmed.starts_with("Option<") && trimmed.ends_with('>') {
        let inner = &trimmed[7..trimmed.len() - 1];
        let inner_types = parse_rust_types_to_lua(inner);
        for t in inner_types {
            result.push(format!("{}?", t));
        }
        return result;
    }

    // Handle tuple types: (Type1, Type2)
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let mut depth = 0;
        let mut current = String::new();
        for c in inner.chars() {
            match c {
                '<' | '(' => {
                    depth += 1;
                    current.push(c);
                }
                '>' | ')' => {
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 => {
                    let t = rust_type_to_lua(current.trim());
                    result.push(t);
                    current.clear();
                }
                _ => current.push(c),
            }
        }
        if !current.is_empty() {
            let t = rust_type_to_lua(current.trim());
            result.push(t);
        }
        return result;
    }

    // Single type
    result.push(rust_type_to_lua(trimmed));
    result
}

/// Convert a single Rust type to Lua/EmmyLua type annotation.
fn rust_type_to_lua(rust_type: &str) -> String {
    match rust_type {
        "u64" | "u32" | "u16" | "u8" | "i64" | "i32" | "i16" | "i8" | "usize" | "isize" => {
            "integer".to_string()
        }
        "f64" | "f32" => "number".to_string(),
        "String" | "&str" => "string".to_string(),
        "bool" => "boolean".to_string(),
        "Vec<String>" => "string[]".to_string(),
        "Table" => "table".to_string(),
        "Value" => "any".to_string(),
        "LuaFunction" => "fun(...)".to_string(),
        "Function" => "fun(...)".to_string(), // mlua::Function alias
        t if t.starts_with("Option<") => {
            let inner = &t[7..t.len() - 1];
            format!("{}?", rust_type_to_lua(inner))
        }
        t if t.starts_with("Vec<") => {
            let inner = &t[4..t.len() - 1];
            format!("{}[]", rust_type_to_lua(inner))
        }
        _ => "any".to_string(),
    }
}

#[derive(Debug)]
struct LuaFunctionDef {
    name: String,
    signature: String,
}

/// Static fallback type definitions when parsing fails.
fn generate_lua_types_static() -> String {
    let mut output = String::from(LUA_TYPE_HEADER);

    output.push_str("---@class PokeConApi\n");
    output.push_str("---@field wait fun(ms: integer) @Sleep for the given milliseconds\n");
    output.push_str(
        "---@field press_button fun(button: string, duration: integer) @Press a single button\n",
    );
    output.push_str("---@field press fun(buttons: string[], opts?: { duration?: integer, wait?: integer }) @Press multiple buttons\n");
    output.push_str(
        "---@field hold fun(buttons: string[], opts?: { wait?: integer }) @Hold buttons down\n",
    );
    output.push_str("---@field hold_end fun(buttons: string[]) @Release held buttons\n");
    output.push_str(
        "---@field move_stick fun(stick: string, x: number, y: number) @Move analog stick\n",
    );
    output.push_str("---@field log fun(msg: string) @Log a message to output panel\n");
    output.push_str("---@field print_s fun(msg: string) @Print to Output #1\n");
    output.push_str("---@field print_t fun(msg: string) @Print to Output #2\n");
    output.push_str("---@field print_t1 fun(msg: string) @Alias for print_s\n");
    output.push_str("---@field print_t2 fun(msg: string) @Alias for print_t\n");
    output.push_str(
        "---@field on fun(event_name: string, callback: fun(...)) @Register event handler\n",
    );
    output.push_str("---@field off fun(event_name: string) @Remove event handlers\n");
    output.push_str("---@field emit fun(event_name: string, data?: any) @Emit an event\n");
    output.push_str(
        "---@field define_event fun(name: string, schema?: table) @Define a custom event schema\n",
    );
    output.push_str("---@field autocmd fun(event_name: string, opts?: { callback?: string, group?: string }) @Register autocmd\n");
    output.push_str(
        "---@field discord_text fun(content: string, index?: integer) @Send Discord message\n",
    );
    output.push_str(
        "---@field line_text fun(txt: string, token?: string) @Send LINE message (deprecated)\n",
    );
    output.push_str(
        "---@field direct_serial fun(cmd: string, wait_ms?: integer) @Send raw serial command\n",
    );
    output.push('\n');

    output.push_str("---@type PokeConApi\n");
    output.push_str("pokecon = {}\n");
    output.push('\n');

    output.push_str("---@class PokeConPressOpts\n");
    output.push_str(
        "---@field duration? integer @Button press duration in milliseconds (default: 100)\n",
    );
    output.push_str(
        "---@field wait? integer @Wait time after press in milliseconds (default: 100)\n",
    );
    output.push('\n');

    output.push_str("---@class PokeConHoldOpts\n");
    output.push_str("---@field wait? integer @Hold duration in milliseconds (default: 100)\n");
    output.push('\n');

    output.push_str("---@class PokeConAutocmdOpts\n");
    output.push_str("---@field callback? string @Callback function reference\n");
    output.push_str("---@field group? string @Autocmd group name\n");
    output.push('\n');

    output.push_str("---@alias PokeConEventName string | \"CommandStartPre\" | \"CommandStartPost\" | \"CommandEndPre\" | \"CommandEndPost\" | \"EncounterShiny\" | \"Error\"\n");
    output.push('\n');

    output.push_str("---@class PokeConEvent\n");
    output.push_str("---@field name string @Event name\n");
    output.push_str("---@field data? table @Event payload\n");
    output.push_str("---@field propagation_stopped boolean @Whether propagation was stopped\n");
    output.push('\n');

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_type_to_lua() {
        assert_eq!(rust_type_to_lua("u64"), "integer");
        assert_eq!(rust_type_to_lua("String"), "string");
        assert_eq!(rust_type_to_lua("f64"), "number");
        assert_eq!(rust_type_to_lua("bool"), "boolean");
        assert_eq!(rust_type_to_lua("Vec<String>"), "string[]");
        assert_eq!(rust_type_to_lua("LuaFunction"), "fun(...)");
        assert_eq!(rust_type_to_lua("Option<u64>"), "integer?");
        assert_eq!(rust_type_to_lua("Option<String>"), "string?");
    }

    #[test]
    fn test_parse_rust_types_to_lua() {
        let types = parse_rust_types_to_lua("(String, u64)");
        assert_eq!(types, vec!["string", "integer"]);

        let types = parse_rust_types_to_lua("(String, Option<u64>)");
        assert_eq!(types, vec!["string", "integer?"]);
    }

    #[test]
    fn test_parse_tuple_elements() {
        let elems = parse_tuple_elements("(a, b, c)");
        assert_eq!(elems, vec!["a", "b", "c"]);

        let elems = parse_tuple_elements("(event_name, callback)");
        assert_eq!(elems, vec!["event_name", "callback"]);
    }

    #[test]
    fn test_sanitize_param_name() {
        assert_eq!(sanitize_param_name("_lua"), "lua");
        assert_eq!(sanitize_param_name("event_name"), "event_name");
        assert_eq!(sanitize_param_name("  _arg  "), "arg");
    }

    #[test]
    fn test_parse_rust_params_to_lua() {
        let params = parse_rust_params_to_lua("(event_name, callback)", "(String, LuaFunction)");
        assert_eq!(params, vec!["event_name: string", "callback: fun(...)"]);

        let params = parse_rust_params_to_lua("(buttons, opts)", "(Vec<String>, Option<Table>)");
        assert_eq!(params, vec!["buttons: string[]", "opts: table?"]);
    }

    #[test]
    fn test_extract_function_signature_tuple() {
        let lines =
            vec!["lua.create_function(move |lua, (event_name, callback): (String, LuaFunction)| {"];
        let mut i = 0;
        let result = extract_function_signature(&lines, &mut i, "on");
        assert!(result.is_some());
        let def = result.unwrap();
        assert_eq!(def.name, "on");
        assert!(def.signature.contains("event_name: string"));
        assert!(def.signature.contains("callback: fun(...)"));
    }

    #[test]
    fn test_extract_function_signature_single() {
        let lines = vec!["lua.create_function(|_, msg: String| {"];
        let mut i = 0;
        let result = extract_function_signature(&lines, &mut i, "log");
        assert!(result.is_some());
        let def = result.unwrap();
        assert_eq!(def.name, "log");
        assert!(def.signature.contains("msg: string"));
    }

    #[test]
    fn test_extract_function_signature_move_single() {
        // move |_, name: Type| pattern (used by "off")
        let lines = vec!["lua.create_function(move |_, event_name: String| {"];
        let mut i = 0;
        let result = extract_function_signature(&lines, &mut i, "off");
        assert!(result.is_some());
        let def = result.unwrap();
        assert_eq!(def.name, "off");
        assert!(def.signature.contains("event_name: string"));
    }

    #[test]
    fn test_extract_function_signature_vec_param() {
        // Vec<String> single param (used by "hold_end")
        let lines = vec!["lua.create_function(|_, buttons: Vec<String>| {"];
        let mut i = 0;
        let result = extract_function_signature(&lines, &mut i, "hold_end");
        assert!(result.is_some());
        let def = result.unwrap();
        assert_eq!(def.name, "hold_end");
        assert!(def.signature.contains("buttons: string[]"));
    }

    #[test]
    fn test_generate_lua_types_from_source_end_to_end() {
        let api_source = r#"
        api.set(
            "wait",
            lua.create_function(|_, ms: u64| {
                Ok(())
            })?,
        )?;

        api.set(
            "press",
            lua.create_function(|_, (buttons, opts): (Vec<String>, Option<Table>)| {
                Ok(())
            })?,
        )?;

        api.set(
            "on",
            lua.create_function(move |lua, (event_name, callback): (String, LuaFunction)| {
                Ok(())
            })?,
        )?;

        api.set(
            "log",
            lua.create_function(|_, msg: String| {
                Ok(())
            })?,
        )?;
        "#;

        let output = generate_lua_types_from_source(api_source);

        // Verify all functions are present
        assert!(
            output.contains("---@field wait fun(ms: integer)"),
            "wait function missing"
        );
        assert!(
            output.contains("---@field press fun(buttons: string[], opts: table?)"),
            "press function missing"
        );
        assert!(
            output.contains("---@field on fun(event_name: string, callback: fun(...))"),
            "on function missing"
        );
        assert!(
            output.contains("---@field log fun(msg: string)"),
            "log function missing"
        );

        // Verify global declaration
        assert!(output.contains("---@type PokeConApi"));
        assert!(output.contains("pokecon = {}"));
    }

    #[test]
    fn test_generate_lua_types_static_fallback() {
        let output = generate_lua_types_static();
        assert!(output.contains("---@class PokeConApi"));
        assert!(output.contains("---@field wait fun(ms: integer)"));
        assert!(output.contains("---@field press_button fun(button: string, duration: integer)"));
        assert!(output.contains("---@type PokeConApi"));
        assert!(output.contains("pokecon = {}"));
    }
}
