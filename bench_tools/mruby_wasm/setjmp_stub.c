// Stub setjmp/longjmp for wasm32 without exception handling.
// setjmp always returns 0 (happy path). longjmp aborts since we can't
// truly restore execution context without wasm exceptions.
// This is sufficient for mruby tool execution where errors are rare.

#include <stdlib.h>

typedef unsigned long jmp_buf[8];

int setjmp(jmp_buf env) {
    (void)env;
    return 0;
}

void longjmp(jmp_buf env, int val) {
    (void)env;
    (void)val;
    abort();
}

int _setjmp(jmp_buf env) {
    return setjmp(env);
}

void _longjmp(jmp_buf env, int val) {
    longjmp(env, val);
}
