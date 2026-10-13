//! A Lua module for LuaTeX: `require("chartgen_luatex").round(spec, options, measurements)`.
//!
//! The in-process route of `chartgen.sty`: the engine runs inside the LuaLaTeX run, and the
//! document measures labels in place, so a chart is drawn in a single pass with no files and
//! no processes. `round` returns the reply of `chartgen_tex::reply`: `measure` or `done`, then
//! TeX; or `error` and a message. The Lua API is the one LuaTeX itself exports (Lua 5.3),
//! resolved when LuaTeX loads the module, which it does only with shell escape enabled.
use std::ffi::{c_char, c_int, c_void};

#[allow(non_camel_case_types)]
type lua_State = c_void;

unsafe extern "C" {
    fn luaL_checklstring(state: *mut lua_State, arg: c_int, len: *mut usize) -> *const c_char;
    fn lua_pushlstring(state: *mut lua_State, s: *const c_char, len: usize) -> *const c_char;
    fn lua_pushcclosure(
        state: *mut lua_State,
        f: unsafe extern "C" fn(*mut lua_State) -> c_int,
        n: c_int,
    );
    fn lua_createtable(state: *mut lua_State, narr: c_int, nrec: c_int);
    fn lua_setfield(state: *mut lua_State, idx: c_int, k: *const c_char);
}

/// Argument `index` as text. Lua strings are bytes; the engine reads UTF-8
unsafe fn argument(state: *mut lua_State, index: c_int) -> String {
    let mut len = 0usize;
    // luaL_checklstring raises a Lua error, rather than returning, when the argument is missing
    let data = unsafe { luaL_checklstring(state, index, &mut len) };
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) };
    String::from_utf8_lossy(bytes).into_owned()
}

unsafe extern "C" fn round(state: *mut lua_State) -> c_int {
    let (spec, options, measurements) =
        unsafe { (argument(state, 1), argument(state, 2), argument(state, 3)) };
    // A panic must not unwind into LuaTeX
    let reply = std::panic::catch_unwind(|| chartgen_tex::reply(&spec, &options, &measurements))
        .unwrap_or_else(|_| "error\nthe chart engine failed unexpectedly".to_string());
    unsafe { lua_pushlstring(state, reply.as_ptr().cast(), reply.len()) };
    1
}

/// `require("chartgen_luatex")`: a table with `round`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn luaopen_chartgen_luatex(state: *mut lua_State) -> c_int {
    unsafe {
        lua_createtable(state, 0, 1);
        lua_pushcclosure(state, round, 0);
        lua_setfield(state, -2, c"round".as_ptr());
    }
    1
}
