/* Minimal setjmp/longjmp for WASM without EH. */
#include <stdlib.h>
#include <setjmp.h>

int setjmp(jmp_buf env) {
    (void)env;
    return 0;
}

void longjmp(jmp_buf env, int val) {
    (void)env;
    (void)val;
    abort();
}

int sigsetjmp(sigjmp_buf env, int savesigs) {
    (void)env;
    (void)savesigs;
    return 0;
}

void siglongjmp(sigjmp_buf env, int val) {
    (void)env;
    (void)val;
    abort();
}
