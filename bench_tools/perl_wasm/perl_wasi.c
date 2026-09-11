/*
 * Perl WASM frontend for shared interpreter core.
 * Thin wrapper: defines Perl dialect, exports WASI functions.
 */

#include "../interp_core/interp.c"

#define EXEC_SLOT_SIZE (512 * 1024)
static char exec_slot[EXEC_SLOT_SIZE];

static InterpDialect perl_dialect = {
    .prefix = "perl_wasi",
    .line_comment_hash = 1,
    .line_comment_slash = 0,
    .block_comment = 0,
    .var_prefix = '$',
    .assign_arrow = 0,
    .semicolon_required = 1,
    .interp_style = 1,
    .concat_dot = 1,
    .brace_blocks = 1,
    .skip_prefix = NULL,
    .func_keyword = "sub",
    .return_parens = 0,
    .type_keywords = {NULL},
    .type_annotations = 0,
    .print_keywords = {"print", "say", NULL},
};

int perl_wasi_init(void) {
    interp_init(&perl_dialect);
    return 0;
}

int perl_wasi_exec(const char *src_ptr, int src_len) {
    int len = src_len;
    if (len >= EXEC_SLOT_SIZE) len = EXEC_SLOT_SIZE - 1;
    memcpy(exec_slot, src_ptr, len);
    exec_slot[len] = '\0';
    return interp_exec(exec_slot);
}

const char *perl_wasi_get_output(void) {
    return interp_get_output();
}

int perl_wasi_get_output_len(void) {
    return (int)interp_get_output_len();
}

int perl_wasi_register_tool(const char *name_ptr, int name_len,
                            const char *src_ptr, int src_len) {
    interp_register_tool(name_ptr, name_len, src_ptr, src_len);
    return 0;
}

int perl_wasi_call_tool(const char *args_ptr, int args_len,
                        const char *name_ptr, int name_len) {
    return interp_call_tool(args_ptr, args_len, name_ptr, name_len);
}

void perl_wasi_destroy(void) {
    interp_destroy();
}
