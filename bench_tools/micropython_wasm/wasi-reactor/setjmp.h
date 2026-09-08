/* Custom setjmp.h for WASI reactor — avoids the wasi-sdk #error that requires
 * -mllvm -wasm-enable-sjlj (WASM exceptions proposal).
 *
 * We provide stub setjmp/longjmp: setjmp always returns 0, longjmp aborts.
 * This means Python exceptions cannot be caught, but normal execution works.
 * Wasmer Cranelift cannot compile WASM exception handling opcodes.
 */
#ifndef _WASI_REACTOR_SETJMP_H
#define _WASI_REACTOR_SETJMP_H

#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned long jmp_buf[32];

int setjmp(jmp_buf env);
void longjmp(jmp_buf env, int val);

#ifdef __cplusplus
}
#endif

#endif /* _WASI_REACTOR_SETJMP_H */
