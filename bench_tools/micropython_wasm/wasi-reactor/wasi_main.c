/*
 * MicroPython WASI entry point.
 *
 * Provides a minimal API for embedding MicroPython in a WASM host:
 *   mp_wasi_init(pystack_size, heap_size) — initialize the interpreter
 *   mp_wasi_exec(src, len)               — execute Python source, returns 0 on success
 *   mp_wasi_get_output()                 — get captured output as C string
 *   mp_wasi_get_output_len()             — get output length
 *   mp_wasi_destroy()                    — tear down the interpreter
 *
 * Output is captured from stdout (print() calls). Errors are captured too.
 */

#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <setjmp.h>

#include "py/builtin.h"
#include "py/compile.h"
#include "py/runtime.h"
#include "py/gc.h"
#include "py/mperrno.h"
#include "shared/runtime/pyexec.h"

#include "lexer_dedent.h"

/* Custom setjmp/longjmp to avoid WASM exception handling opcodes.
 * Wasmer Cranelift cannot compile WASM exceptions, so we provide stub implementations.
 * setjmp always returns 0 (normal execution path).
 * longjmp aborts since we can't unwind the stack without WASM exceptions.
 * This is acceptable for a tool server where Python exceptions are rare.
 */
int setjmp(jmp_buf env) {
    (void)env;
    return 0;
}

void longjmp(jmp_buf env, int val) {
    (void)env;
    (void)val;
    fprintf(stderr, "FATAL: Python exception raised but stack unwinding not supported\\n");
    abort();
}

/* Output capture buffer — results from Python print() and return values. */
static char *output_buf = NULL;
static size_t output_len = 0;
static size_t output_cap = 0;

/* Random seed for mp_js_random_u32. */
static uint32_t random_seed = 12345;

uint32_t mp_js_random_u32(void) {
    random_seed ^= random_seed << 13;
    random_seed ^= random_seed >> 17;
    random_seed ^= random_seed << 5;
    return random_seed;
}

/* Time functions using WASI clock. */
static uint64_t wasi_clock_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

int mp_js_ticks_ms(void) {
    return (int)(wasi_clock_ns() / 1000000ULL);
}

double mp_js_time_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return (double)ts.tv_sec * 1000.0 + (double)ts.tv_nsec / 1000000.0;
}

uint64_t mp_hal_time_ms(void) {
    return (uint64_t)mp_js_time_ms();
}

uint64_t mp_hal_time_ns(void) {
    return mp_hal_time_ms() * 1000000ULL;
}

mp_uint_t mp_hal_ticks_ms(void) {
    return (mp_uint_t)mp_js_ticks_ms();
}

mp_uint_t mp_hal_ticks_us(void) {
    return (mp_uint_t)mp_js_ticks_ms() * 1000;
}

mp_uint_t mp_hal_ticks_cpu(void) {
    return 0;
}

void mp_hal_delay_ms(mp_uint_t ms) {
    (void)ms;
}

void mp_hal_delay_us(mp_uint_t us) {
    (void)us;
}

extern int mp_interrupt_char;

int mp_hal_get_interrupt_char(void) {
    return mp_interrupt_char;
}

void mp_js_hook(void) {
    /* no-op in WASI */
}

/* Output capture: redirect stdout to our buffer. */
static void output_append(const char *str, size_t len) {
    if (output_len + len > output_cap) {
        size_t new_cap = output_cap == 0 ? 4096 : output_cap * 2;
        while (new_cap < output_len + len) new_cap *= 2;
        output_buf = realloc(output_buf, new_cap);
        output_cap = new_cap;
    }
    memcpy(output_buf + output_len, str, len);
    output_len += len;
    output_buf[output_len] = '\0';
}

static void output_reset(void) {
    if (output_buf) output_buf[0] = '\0';
    output_len = 0;
}

/* stdout write redirect */
static void stdout_print_strn(void *env, const char *str, size_t len) {
    (void)env;
    output_append(str, len);
}

const mp_print_t mp_stderr_print = {NULL, stdout_print_strn};

mp_uint_t mp_hal_stdout_tx_strn(const char *str, size_t len) {
    output_append(str, len);
    return len;
}

/* GC: deferred collection — gc_collect() just sets a flag; actual collection
 * happens at the top level (in mp_wasi_exec) where there are no stack roots. */
static bool gc_collect_pending = false;

size_t gc_get_max_new_split(void) {
    return 128 * 1024 * 1024;
}

void gc_collect(void) {
    gc_collect_pending = true;
}

static void gc_collect_top_level(void) {
    if (gc_collect_pending) {
        gc_collect_pending = false;
        gc_collect_start();
        gc_collect_end();
    }
}

/* NLR jump fail — should never be reached. */
void nlr_jump_fail(void *val) {
    (void)val;
    while (1) {}
}

void NORETURN __fatal_error(const char *msg) {
    (void)msg;
    while (1) {}
}

#ifndef NDEBUG
void MP_WEAK __assert_func(const char *file, int line, const char *func, const char *expr) {
    (void)func;
    fprintf(stderr, "Assertion '%s' failed, at %s:%d\n", expr, file, line);
    __fatal_error("Assertion failed");
}
#endif

/* File stubs (no VFS). */
#if !MICROPY_VFS
mp_lexer_t *mp_lexer_new_from_file(qstr filename) {
    (void)filename;
    mp_raise_OSError(MP_ENOENT);
}

mp_import_stat_t mp_import_stat(const char *path) {
    (void)path;
    return MP_IMPORT_STAT_NO_EXIST;
}

mp_obj_t mp_builtin_open(size_t n_args, const mp_obj_t *args, mp_map_t *kwargs) {
    (void)n_args; (void)args; (void)kwargs;
    return mp_const_none;
}
MP_DEFINE_CONST_FUN_OBJ_KW(mp_builtin_open_obj, 1, mp_builtin_open);
#endif

/* ===== Public API ===== */

/* Initialize MicroPython interpreter.
 * pystack_size: size of Python stack in objects (e.g. 16384)
 * heap_size: GC heap size in bytes (e.g. 131072 = 128KB)
 */
int mp_wasi_init(int pystack_size, int heap_size) {
    output_reset();

    mp_obj_t *pystack = (mp_obj_t *)malloc(pystack_size * sizeof(mp_obj_t));
    if (!pystack) return -1;
    mp_pystack_init(pystack, pystack + pystack_size);

    char *heap = (char *)malloc(heap_size);
    if (!heap) { free(pystack); return -2; }
    gc_init(heap, heap + heap_size);

    MP_STATE_MEM(gc_alloc_threshold) = 16 * 1024 / MICROPY_BYTES_PER_GC_BLOCK;

    mp_init();

    return 0;
}

/* Execute Python source code.
 * Returns 0 on success, -1 on exception.
 * Output (print statements) is captured and available via mp_wasi_get_output().
 * On exception, the error message is captured in output.
 */
int mp_wasi_exec(const char *src, size_t len) {
    output_reset();

    mp_print_t stdout_print = {NULL, stdout_print_strn};

    nlr_buf_t nlr;
    if (nlr_push(&nlr) == 0) {
        mp_lexer_t *lex = mp_lexer_new_from_str_len_dedent(MP_QSTR__lt_stdin_gt_, src, len, 0);
        qstr source_name = lex->source_name;
        mp_parse_tree_t parse_tree = mp_parse(lex, MP_PARSE_FILE_INPUT);
        mp_obj_t module_fun = mp_compile(&parse_tree, source_name, false);
        mp_call_function_0(module_fun);
        nlr_pop();
        gc_collect_top_level();
        return 0;
    } else {
        mp_obj_print_exception(&stdout_print, (mp_obj_t)nlr.ret_val);
        gc_collect_top_level();
        return -1;
    }
}

/* Execute Python source and return result of expression evaluation.
 * For tool use: wraps source as `result = (<expr>)` and captures repr.
 */
int mp_wasi_exec_expr(const char *src, size_t len) {
    output_reset();

    mp_print_t stdout_print = {NULL, stdout_print_strn};

    nlr_buf_t nlr;
    if (nlr_push(&nlr) == 0) {
        mp_lexer_t *lex = mp_lexer_new_from_str_len_dedent(MP_QSTR__lt_stdin_gt_, src, len, 0);
        qstr source_name = lex->source_name;
        mp_parse_tree_t parse_tree = mp_parse(lex, MP_PARSE_SINGLE_INPUT);
        mp_obj_t module_fun = mp_compile(&parse_tree, source_name, false);
        mp_obj_t ret = mp_call_function_0(module_fun);
        if (ret != mp_const_none) {
            mp_obj_print_helper(&stdout_print, ret, PRINT_REPR);
        }
        nlr_pop();
        gc_collect_top_level();
        return 0;
    } else {
        mp_obj_print_exception(&stdout_print, (mp_obj_t)nlr.ret_val);
        gc_collect_top_level();
        return -1;
    }
}

const char *mp_wasi_get_output(void) {
    return output_buf ? output_buf : "";
}

size_t mp_wasi_get_output_len(void) {
    return output_len;
}

void mp_wasi_destroy(void) {
    mp_deinit();
    free(output_buf);
    output_buf = NULL;
    output_len = 0;
    output_cap = 0;
}

/* malloc/free are exported by default in WASI — no wrappers needed. */
