/*
 * Julia WASI reactor wrapper for VELOCITY-MCP.
 * Note: Julia WASM support is experimental. See https://github.com/JuliaLang/julia/issues/35151
 * This template assumes Julia compiled to WASM via experimental toolchain.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define OUTPUT_BUF_SIZE (256 * 1024)
static char output_buf[OUTPUT_BUF_SIZE];
static size_t output_len = 0;

#define ARGS_BUF_SIZE 4096
static char args_buf[ARGS_BUF_SIZE];
static size_t args_len = 0;

static int julia_initialized = 0;

static void output_reset(void) {
    output_len = 0;
    output_buf[0] = '\0';
}

static void output_append(const char *data, size_t len) {
    if (output_len + len >= OUTPUT_BUF_SIZE) {
        len = OUTPUT_BUF_SIZE - output_len - 1;
    }
    if (len > 0) {
        memcpy(output_buf + output_len, data, len);
        output_len += len;
        output_buf[output_len] = '\0';
    }
}

/*
 * TODO: Implement Julia execution
 * Julia WASM is experimental. Options:
 * 1. Use Julia's experimental WASM backend (if available)
 * 2. Compile Julia to C via PackageCompiler, then to WASM
 * 3. Use a Julia interpreter compiled to WASM
 * 
 * See: https://github.com/JuliaLang/julia/issues/35151
 */

int julia_wasi_init(void) {
    if (julia_initialized) return 0;
    
    /* TODO: Initialize Julia runtime
     * jl_init();
     */
    
    julia_initialized = 1;
    output_reset();
    return 0;
}

int julia_wasi_exec(const char *src, size_t len) {
    if (!julia_initialized) return -1;
    
    output_reset();
    
    /* TODO: Execute Julia source
     * jl_eval_string(src);
     */
    
    output_append("# Julia source: ", 15);
    output_append(src, len);
    
    return 0;
}

const char *julia_wasi_get_output(void) {
    return output_buf;
}

size_t julia_wasi_get_output_len(void) {
    return output_len;
}

int julia_wasi_set_args(const char *ptr, size_t len) {
    if (len >= ARGS_BUF_SIZE) len = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, ptr, len);
    args_buf[len] = '\0';
    args_len = len;
    return 0;
}

int julia_wasi_register_tool(const char *name_ptr, size_t name_len,
                             const char *src_ptr, size_t src_len) {
    (void)name_ptr; (void)name_len;
    (void)src_ptr; (void)src_len;
    return 0;
}

int julia_wasi_call_tool(const char *args_ptr, size_t args_n,
                         const char *name_ptr, size_t name_len) {
    if (!julia_initialized) return -1;
    
    if (args_n >= ARGS_BUF_SIZE) args_n = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, args_ptr, args_n);
    args_buf[args_n] = '\0';
    
    output_reset();
    
    (void)name_ptr; (void)name_len;
    
    return 0;
}

void julia_wasi_destroy(void) {
    if (julia_initialized) {
        /* TODO: jl_atexit_hook(0); */
        julia_initialized = 0;
    }
}
