/*
 * Custom setjmp/longjmp stubs for Lua on WASM/WASI.
 * Wasmer Cranelift cannot compile the wasm32 exception handling opcodes
 * that wasi-sdk's setjmp/longjmp require. Lua uses setjmp/longjmp for
 * error handling (luaD_throw), so we provide stubs that work for the
 * common case (no exceptions) and abort on actual errors.
 */

#ifndef _LUA_WASI_SETJMP_H
#define _LUA_WASI_SETJMP_H

typedef int jmp_buf[64];

static inline int setjmp(jmp_buf env) {
    (void)env;
    return 0;
}

static inline void longjmp(jmp_buf env, int val) {
    (void)env;
    (void)val;
    __builtin_trap();
}

#endif
