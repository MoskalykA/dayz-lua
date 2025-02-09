#![cfg(windows)]
#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use lazy_static::lazy_static;
use mlua::{
    ffi::{luaL_checkstring, lua_type, LUA_TSTRING},
    lua_State, Function, RegistryKey,
};
use once_cell::sync::{Lazy, OnceCell};
use retour::GenericDetour;
use std::{collections::HashMap, ffi::CStr, os::raw::c_void, sync::Mutex};
use windows::{
    core::PCSTR,
    Win32::{
        Foundation::{BOOL, HANDLE},
        System::{
            Console::AllocConsole,
            LibraryLoader::GetModuleHandleA,
            SystemServices::{
                DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH, DLL_THREAD_ATTACH, DLL_THREAD_DETACH,
            },
        },
    },
};

static LUA: OnceCell<Mutex<mlua::Lua>> = OnceCell::new();

type fn_Print = unsafe extern "fastcall" fn(a1: i64, a2: i64) -> i64;
type fn_AddFunction =
    unsafe extern "fastcall" fn(a1: i64, a2: i64, a3: i64, a4: i64, a5: u32) -> i64;

lazy_static! {
    static ref Hooks: Mutex<HashMap<String, OnceCell<RegistryKey>>> = Mutex::new(HashMap::new());
}

static hook_AddFunction: Lazy<GenericDetour<fn_AddFunction>> = Lazy::new(|| {
    let handle = unsafe { GetModuleHandleA(PCSTR::null()).unwrap() };
    let ori: fn_AddFunction =
        unsafe { std::mem::transmute::<usize, fn_AddFunction>(handle.0 as usize + 0x2A00C0) };
    return unsafe { GenericDetour::new(ori, our_AddFunction).unwrap() };
});

unsafe extern "fastcall" fn our_AddFunction(a1: i64, a2: i64, a3: i64, a4: i64, a5: u32) -> i64 {
    let name = CStr::from_ptr(a3 as *const i8);
    let hooks = Hooks.lock().unwrap();
    let r = hooks.get("test").unwrap().get().unwrap();
    LUA.get()
        .unwrap()
        .lock()
        .unwrap()
        .registry_value::<Function>(r)
        .unwrap()
        .call::<_, ()>(name.to_str().unwrap())
        .unwrap();

    let result = hook_AddFunction.call(a1, a2, a3, a4, a5);
    result
}

unsafe extern "C-unwind" fn print(state: *mut lua_State) -> i32 {
    let handle = GetModuleHandleA(PCSTR::null()).unwrap();
    let ori: fn_Print = std::mem::transmute::<usize, fn_Print>(handle.0 as usize + 0x71D250);
    match lua_type(state, 1) {
        LUA_TSTRING => {
            ori(1, luaL_checkstring(state, 1) as i64);
        }
        _ => {}
    }

    1
}

#[no_mangle]
unsafe extern "system" fn DllMain(_hinst: HANDLE, reason: u32, _reserved: *mut c_void) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            AllocConsole().unwrap();

            hook_AddFunction.enable().unwrap();

            let lua = LUA
                .get_or_init(|| Mutex::new(mlua::Lua::new()))
                .lock()
                .unwrap();

            let print = lua.create_c_function(print).unwrap();
            lua.globals().raw_set("print", print).unwrap();

            let hook_add = lua
                .create_function(move |lua, (name, callback): (String, Function)| {
                    let c = OnceCell::new();
                    c.set(lua.create_registry_value(callback).unwrap()).unwrap();

                    Hooks.lock().unwrap().insert(name, c);

                    Ok(())
                })
                .unwrap();

            let hook_run = lua
                .create_function(move |lua, name: String| {
                    let hooks = Hooks.lock().unwrap();
                    let r = hooks.get(&name).unwrap().get().unwrap();
                    lua.registry_value::<Function>(r)
                        .unwrap()
                        .call::<_, ()>(())
                        .unwrap();

                    Ok(())
                })
                .unwrap();

            let hooks = lua
                .create_table_from(vec![("Add", hook_add), ("Run", hook_run)])
                .unwrap();

            lua.globals().raw_set("hooks", hooks).unwrap();
            lua.load(
                r#"
                hooks.Add("test", function(a)
                    print(a)
                end)
            "#,
            )
            .exec()
            .unwrap();
        }
        DLL_PROCESS_DETACH => {
            println!("detaching");
        }
        DLL_THREAD_ATTACH => {}
        DLL_THREAD_DETACH => {}
        _ => {}
    };

    BOOL::from(true)
}
