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

/* ===== Pre-compiled wrapper protocol ===== */

/* Args slot: 4KB buffer for passing JSON args without code interpolation */
#define ARGS_BUF_SIZE 4096
static char args_buf[ARGS_BUF_SIZE];
static size_t args_len = 0;

/* Tool registry: store pre-compiled wrapper module functions */
#define MAX_TOOLS 64
static struct {
    char name[64];
    mp_obj_t compiled_wrapper;
} tool_registry[MAX_TOOLS];
static int tool_count = 0;

/* Minimal JSON parser producing mp_obj_t values */
typedef struct {
    const char *s;
    size_t pos;
    size_t len;
} MpJsonParser;

static void mjp_skip_ws(MpJsonParser *p) {
    while (p->pos < p->len) {
        char c = p->s[p->pos];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') p->pos++;
        else break;
    }
}

static mp_obj_t mjp_parse_value(MpJsonParser *p);

static mp_obj_t mjp_parse_string(MpJsonParser *p) {
    if (p->pos >= p->len || p->s[p->pos] != '"') return mp_const_none;
    p->pos++;
    vstr_t vstr;
    vstr_init(&vstr, 32);
    while (p->pos < p->len && p->s[p->pos] != '"') {
        char c = p->s[p->pos];
        if (c == '\\') {
            p->pos++;
            if (p->pos >= p->len) break;
            c = p->s[p->pos];
            switch (c) {
                case '"': vstr_add_char(&vstr, '"'); break;
                case '\\': vstr_add_char(&vstr, '\\'); break;
                case '/': vstr_add_char(&vstr, '/'); break;
                case 'b': vstr_add_char(&vstr, '\b'); break;
                case 'f': vstr_add_char(&vstr, '\f'); break;
                case 'n': vstr_add_char(&vstr, '\n'); break;
                case 'r': vstr_add_char(&vstr, '\r'); break;
                case 't': vstr_add_char(&vstr, '\t'); break;
                case 'u': p->pos += 4; vstr_add_char(&vstr, '?'); break;
                default: vstr_add_char(&vstr, c); break;
            }
        } else {
            vstr_add_char(&vstr, c);
        }
        p->pos++;
    }
    if (p->pos < p->len) p->pos++;
    mp_obj_t result = mp_obj_new_str(vstr.buf, vstr.len);
    vstr_clear(&vstr);
    return result;
}

static mp_obj_t mjp_parse_number(MpJsonParser *p) {
    size_t start = p->pos;
    int is_float = 0;
    if (p->pos < p->len && p->s[p->pos] == '-') p->pos++;
    while (p->pos < p->len && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++;
    if (p->pos < p->len && p->s[p->pos] == '.') {
        is_float = 1;
        p->pos++;
        while (p->pos < p->len && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++;
    }
    if (p->pos < p->len && (p->s[p->pos] == 'e' || p->s[p->pos] == 'E')) {
        is_float = 1;
        p->pos++;
        if (p->pos < p->len && (p->s[p->pos] == '+' || p->s[p->pos] == '-')) p->pos++;
        while (p->pos < p->len && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++;
    }
    char buf[64];
    size_t nlen = p->pos - start;
    if (nlen >= sizeof(buf)) nlen = sizeof(buf) - 1;
    memcpy(buf, p->s + start, nlen);
    buf[nlen] = '\0';
    if (is_float) {
        return mp_obj_new_float(strtod(buf, NULL));
    } else {
        return mp_obj_new_int(strtoll(buf, NULL, 10));
    }
}

static mp_obj_t mjp_parse_array(MpJsonParser *p) {
    p->pos++;
    mp_obj_t list = mp_obj_new_list(0, NULL);
    mjp_skip_ws(p);
    if (p->pos < p->len && p->s[p->pos] == ']') { p->pos++; return list; }
    for (;;) {
        mjp_skip_ws(p);
        mp_obj_t item = mjp_parse_value(p);
        mp_obj_list_append(list, item);
        mjp_skip_ws(p);
        if (p->pos < p->len && p->s[p->pos] == ',') { p->pos++; continue; }
        break;
    }
    mjp_skip_ws(p);
    if (p->pos < p->len && p->s[p->pos] == ']') p->pos++;
    return list;
}

static mp_obj_t mjp_parse_object(MpJsonParser *p) {
    p->pos++;
    mp_obj_t dict = mp_obj_new_dict(0);
    mjp_skip_ws(p);
    if (p->pos < p->len && p->s[p->pos] == '}') { p->pos++; return dict; }
    for (;;) {
        mjp_skip_ws(p);
        mp_obj_t key = mjp_parse_string(p);
        mjp_skip_ws(p);
        if (p->pos < p->len && p->s[p->pos] == ':') p->pos++;
        mjp_skip_ws(p);
        mp_obj_t value = mjp_parse_value(p);
        mp_obj_dict_store(dict, key, value);
        mjp_skip_ws(p);
        if (p->pos < p->len && p->s[p->pos] == ',') { p->pos++; continue; }
        break;
    }
    mjp_skip_ws(p);
    if (p->pos < p->len && p->s[p->pos] == '}') p->pos++;
    return dict;
}

static mp_obj_t mjp_parse_value(MpJsonParser *p) {
    mjp_skip_ws(p);
    if (p->pos >= p->len) return mp_const_none;
    char c = p->s[p->pos];
    if (c == '"') return mjp_parse_string(p);
    if (c == '{') return mjp_parse_object(p);
    if (c == '[') return mjp_parse_array(p);
    if (c == 't' && p->pos + 3 < p->len && memcmp(p->s + p->pos, "true", 4) == 0) {
        p->pos += 4;
        return mp_const_true;
    }
    if (c == 'f' && p->pos + 4 < p->len && memcmp(p->s + p->pos, "false", 5) == 0) {
        p->pos += 5;
        return mp_const_false;
    }
    if (c == 'n' && p->pos + 3 < p->len && memcmp(p->s + p->pos, "null", 4) == 0) {
        p->pos += 4;
        return mp_const_none;
    }
    if (c == '-' || (c >= '0' && c <= '9')) return mjp_parse_number(p);
    return mp_const_none;
}

/* get_tool_args() builtin — parses args_buf JSON, returns dict */
static mp_obj_t mp_wasi_get_tool_args(void) {
    MpJsonParser p = { .s = args_buf, .pos = 0, .len = args_len };
    return mjp_parse_value(&p);
}
MP_DEFINE_CONST_FUN_OBJ_0(mp_wasi_get_tool_args_obj, mp_wasi_get_tool_args);

/* JSON encoder for mp_obj_t → output_buf */
static void mp_json_encode_value(mp_obj_t obj, vstr_t *vstr, int depth);

static void mp_json_encode_string(const char *s, size_t len, vstr_t *vstr) {
    vstr_add_char(vstr, '"');
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)s[i];
        switch (c) {
            case '"': vstr_add_str(vstr, "\\\""); break;
            case '\\': vstr_add_str(vstr, "\\\\"); break;
            case '\b': vstr_add_str(vstr, "\\b"); break;
            case '\f': vstr_add_str(vstr, "\\f"); break;
            case '\n': vstr_add_str(vstr, "\\n"); break;
            case '\r': vstr_add_str(vstr, "\\r"); break;
            case '\t': vstr_add_str(vstr, "\\t"); break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    snprintf(buf, sizeof(buf), "\\u%04x", c);
                    vstr_add_str(vstr, buf);
                } else {
                    vstr_add_char(vstr, c);
                }
                break;
        }
    }
    vstr_add_char(vstr, '"');
}

static void mp_json_encode_value(mp_obj_t obj, vstr_t *vstr, int depth) {
    if (depth > 32) {
        vstr_add_str(vstr, "null");
        return;
    }
    if (obj == mp_const_none) {
        vstr_add_str(vstr, "null");
    } else if (obj == mp_const_true) {
        vstr_add_str(vstr, "true");
    } else if (obj == mp_const_false) {
        vstr_add_str(vstr, "false");
    } else if (mp_obj_is_int(obj)) {
        char buf[32];
        snprintf(buf, sizeof(buf), "%lld", (long long)mp_obj_get_int(obj));
        vstr_add_str(vstr, buf);
    } else if (mp_obj_is_float(obj)) {
        char buf[64];
        snprintf(buf, sizeof(buf), "%.17g", mp_obj_get_float(obj));
        vstr_add_str(vstr, buf);
    } else if (mp_obj_is_str(obj)) {
        size_t len;
        const char *s = mp_obj_str_get_data(obj, &len);
        mp_json_encode_string(s, len, vstr);
    } else if (mp_obj_is_type(obj, &mp_type_list)) {
        vstr_add_char(vstr, '[');
        size_t list_len;
        mp_obj_t *list_items;
        mp_obj_list_get(obj, &list_len, &list_items);
        for (size_t i = 0; i < list_len; i++) {
            if (i > 0) vstr_add_char(vstr, ',');
            mp_json_encode_value(list_items[i], vstr, depth + 1);
        }
        vstr_add_char(vstr, ']');
    } else if (mp_obj_is_type(obj, &mp_type_dict)) {
        vstr_add_char(vstr, '{');
        mp_map_t *map = mp_obj_dict_get_map(obj);
        int first = 1;
        for (size_t i = 0; i < map->alloc; i++) {
            if (mp_map_slot_is_filled(map, i)) {
                if (!first) vstr_add_char(vstr, ',');
                first = 0;
                if (mp_obj_is_str(map->table[i].key)) {
                    size_t klen;
                    const char *k = mp_obj_str_get_data(map->table[i].key, &klen);
                    mp_json_encode_string(k, klen, vstr);
                } else {
                    vstr_add_str(vstr, "\"?\"");
                }
                vstr_add_char(vstr, ':');
                mp_json_encode_value(map->table[i].value, vstr, depth + 1);
            }
        }
        vstr_add_char(vstr, '}');
    } else {
        vstr_add_str(vstr, "null");
    }
}

/* set_tool_result(value) builtin — JSON-encodes to output_buf */
static mp_obj_t mp_wasi_set_tool_result(mp_obj_t value) {
    output_reset();
    vstr_t vstr;
    vstr_init(&vstr, 256);
    mp_json_encode_value(value, &vstr, 0);
    output_append(vstr.buf, vstr.len);
    vstr_clear(&vstr);
    return mp_const_none;
}
MP_DEFINE_CONST_FUN_OBJ_1(mp_wasi_set_tool_result_obj, mp_wasi_set_tool_result);

/* mp_wasi_register_tool(name_ptr, name_len, src_ptr, src_len) -> int
 * Compiles wrapper source and stores in tool registry.
 */
int mp_wasi_register_tool(const char *name_ptr, size_t name_len,
                          const char *src_ptr, size_t src_len) {
    if (tool_count >= MAX_TOOLS) return -1;
    if (name_len >= sizeof(tool_registry[0].name)) return -2;

    mp_print_t stdout_print = {NULL, stdout_print_strn};
    nlr_buf_t nlr;
    if (nlr_push(&nlr) == 0) {
        mp_lexer_t *lex = mp_lexer_new_from_str_len_dedent(MP_QSTR__lt_stdin_gt_, src_ptr, src_len, 0);
        qstr source_name = lex->source_name;
        mp_parse_tree_t parse_tree = mp_parse(lex, MP_PARSE_FILE_INPUT);
        mp_obj_t module_fun = mp_compile(&parse_tree, source_name, false);
        nlr_pop();
        gc_collect_top_level();

        memcpy(tool_registry[tool_count].name, name_ptr, name_len);
        tool_registry[tool_count].name[name_len] = '\0';
        tool_registry[tool_count].compiled_wrapper = module_fun;
        tool_count++;
        return 0;
    } else {
        mp_obj_print_exception(&stdout_print, (mp_obj_t)nlr.ret_val);
        gc_collect_top_level();
        return -3;
    }
}

/* mp_wasi_call_tool(name_ptr, name_len) -> int
 * Calls the pre-compiled wrapper for the named tool.
 */
int mp_wasi_call_tool(const char *name_ptr, size_t name_len) {
    output_reset();

    /* Find tool in registry */
    mp_obj_t wrapper = NULL;
    for (int i = 0; i < tool_count; i++) {
        if (strlen(tool_registry[i].name) == name_len &&
            memcmp(tool_registry[i].name, name_ptr, name_len) == 0) {
            wrapper = tool_registry[i].compiled_wrapper;
            break;
        }
    }
    if (wrapper == NULL) {
        output_append("Unknown tool", 12);
        return -1;
    }

    mp_print_t stdout_print = {NULL, stdout_print_strn};
    nlr_buf_t nlr;
    if (nlr_push(&nlr) == 0) {
        mp_call_function_0(wrapper);
        nlr_pop();
        gc_collect_top_level();
        return 0;
    } else {
        mp_obj_print_exception(&stdout_print, (mp_obj_t)nlr.ret_val);
        gc_collect_top_level();
        return -2;
    }
}

/* mp_wasi_set_args(ptr, len) -> int — write args to args_buf */
int mp_wasi_set_args(const char *ptr, size_t len) {
    if (len >= ARGS_BUF_SIZE) len = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, ptr, len);
    args_buf[len] = '\0';
    args_len = len;
    return 0;
}

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

    /* Register tool protocol builtins */
    mp_store_global(qstr_from_str("get_tool_args"), MP_OBJ_FROM_PTR(&mp_wasi_get_tool_args_obj));
    mp_store_global(qstr_from_str("set_tool_result"), MP_OBJ_FROM_PTR(&mp_wasi_set_tool_result_obj));

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
