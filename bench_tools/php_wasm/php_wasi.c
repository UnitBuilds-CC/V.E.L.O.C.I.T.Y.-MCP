/*
 * PHP/WASM frontend for shared interpreter core.
 * Thin wrapper: defines PHP dialect, exports WASI functions.
 */

#include "../interp_core/interp.c"

#define EXEC_SLOT_SIZE (512 * 1024)
static char exec_slot[EXEC_SLOT_SIZE];

static InterpDialect php_dialect = {
    .prefix = "php_wasi",
    .line_comment_hash = 1,
    .line_comment_slash = 1,
    .block_comment = 1,
    .var_prefix = '$',
    .assign_arrow = 0,
    .semicolon_required = 1,
    .interp_style = 1,
    .concat_dot = 1,
    .brace_blocks = 1,
    .skip_prefix = "<?php",
    .func_keyword = "function",
    .return_parens = 0,
    .type_keywords = {NULL},
    .type_annotations = 0,
    .print_keywords = {"echo", "print", NULL},
};

int php_wasi_init(void) {
    interp_init(&php_dialect);
    return 0;
}

int php_wasi_exec(const char *src_ptr, int src_len) {
    int len = src_len;
    if (len >= EXEC_SLOT_SIZE) len = EXEC_SLOT_SIZE - 1;
    memcpy(exec_slot, src_ptr, len);
    exec_slot[len] = '\0';
    return interp_exec(exec_slot);
}

const char *php_wasi_get_output(void) {
    return interp_get_output();
}

int php_wasi_get_output_len(void) {
    return (int)interp_get_output_len();
}

int php_wasi_register_tool(const char *name_ptr, int name_len,
                           const char *src_ptr, int src_len) {
    interp_register_tool(name_ptr, name_len, src_ptr, src_len);
    return 0;
}

int php_wasi_call_tool(const char *args_ptr, int args_len,
                       const char *name_ptr, int name_len) {
    return interp_call_tool(args_ptr, args_len, name_ptr, name_len);
}

void php_wasi_destroy(void) {
    interp_destroy();
}
