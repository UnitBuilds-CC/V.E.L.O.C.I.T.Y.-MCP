/* Minimal setjmp/longjmp for WASM without exception handling.
 * Overrides wasi-sysroot's setjmp.h which requires WASM EH.
 * setjmp always returns 0; longjmp aborts.
 * Ruby exceptions will terminate the interpreter, but normal
 * tool execution (without exceptions) works correctly.
 */
#ifndef _SETJMP_H
#define _SETJMP_H

#ifdef __cplusplus
extern "C" {
#endif

typedef int jmp_buf[1];

int setjmp(jmp_buf) __attribute__((__returns_twice__));
_Noreturn void longjmp(jmp_buf, int);

#define _setjmp setjmp
#define _longjmp longjmp

typedef jmp_buf sigjmp_buf;
int sigsetjmp(sigjmp_buf, int) __attribute__((__returns_twice__));
_Noreturn void siglongjmp(sigjmp_buf, int);

#ifdef __cplusplus
}
#endif

#endif
