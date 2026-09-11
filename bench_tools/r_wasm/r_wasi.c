/*
 * R WASM frontend for shared interpreter core.
 * Thin wrapper: defines R dialect, exports WASI functions.
 */

#include "../interp_core/interp.c"

#define EXEC_SLOT_SIZE (512 * 1024)
static char exec_slot[EXEC_SLOT_SIZE];

static InterpDialect r_dialect = {
    .prefix = "r_wasi",
    .line_comment_hash = 1,
    .line_comment_slash = 0,
    .block_comment = 0,
    .var_prefix = 0,
    .assign_arrow = 1,
    .semicolon_required = 0,
    .interp_style = 0,
    .concat_dot = 0,
    .brace_blocks = 0,
    .skip_prefix = NULL,
    .func_keyword = "function",
    .return_parens = 1,
    .type_keywords = {NULL},
    .type_annotations = 0,
    .print_keywords = {"cat", "print", NULL},
};

int r_wasi_init(void) {
    interp_init(&r_dialect);
    return 0;
}

int r_wasi_exec(const char *src_ptr, int src_len) {
    int len = src_len;
    if (len >= EXEC_SLOT_SIZE) len = EXEC_SLOT_SIZE - 1;
    memcpy(exec_slot, src_ptr, len);
    exec_slot[len] = '\0';
    return interp_exec(exec_slot);
}

const char *r_wasi_get_output(void) {
    return interp_get_output();
}

int r_wasi_get_output_len(void) {
    return (int)interp_get_output_len();
}

int r_wasi_register_tool(const char *name_ptr, int name_len,
                         const char *src_ptr, int src_len) {
    interp_register_tool(name_ptr, name_len, src_ptr, src_len);
    return 0;
}

int r_wasi_call_tool(const char *args_ptr, int args_len,
                     const char *name_ptr, int name_len) {
    return interp_call_tool(args_ptr, args_len, name_ptr, name_len);
}

void r_wasi_destroy(void) {
    interp_destroy();
}
