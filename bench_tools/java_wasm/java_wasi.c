/*
 * Java/Kotlin WASM frontend for shared interpreter core.
 * Thin wrapper: defines Java dialect, exports WASI functions.
 */

#include "../interp_core/interp.c"

#define EXEC_SLOT_SIZE (512 * 1024)
static char exec_slot[EXEC_SLOT_SIZE];

static InterpDialect java_dialect = {
    .prefix = "java_wasi",
    .line_comment_hash = 0,
    .line_comment_slash = 1,
    .block_comment = 1,
    .var_prefix = 0,
    .assign_arrow = 0,
    .semicolon_required = 1,
    .interp_style = 0,
    .concat_dot = 0,
    .brace_blocks = 1,
    .skip_prefix = NULL,
    .func_keyword = NULL,
    .return_parens = 0,
    .type_keywords = {"String", "int", "var", NULL},
    .type_annotations = 1,
    .print_keywords = {"System.out.println", "System.out.print", NULL},
};

int java_wasi_init(void) {
    interp_init(&java_dialect);
    return 0;
}

int java_wasi_exec(const char *src_ptr, int src_len) {
    int len = src_len;
    if (len >= EXEC_SLOT_SIZE) len = EXEC_SLOT_SIZE - 1;
    memcpy(exec_slot, src_ptr, len);
    exec_slot[len] = '\0';
    return interp_exec(exec_slot);
}

const char *java_wasi_get_output(void) {
    return interp_get_output();
}

int java_wasi_get_output_len(void) {
    return (int)interp_get_output_len();
}

int java_wasi_register_tool(const char *name_ptr, int name_len,
                            const char *src_ptr, int src_len) {
    interp_register_tool(name_ptr, name_len, src_ptr, src_len);
    return 0;
}

int java_wasi_call_tool(const char *args_ptr, int args_len,
                        const char *name_ptr, int name_len) {
    return interp_call_tool(args_ptr, args_len, name_ptr, name_len);
}

int java_wasi_call_tool_binary(const char *tlv_ptr, int tlv_len,
                                const char *name_ptr, int name_len) {
    return interp_call_tool_binary(tlv_ptr, tlv_len, name_ptr, name_len);
}

void java_wasi_destroy(void) {
    interp_destroy();
}
