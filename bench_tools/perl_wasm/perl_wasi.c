/*
 * Perl WASI reactor wrapper for VELOCITY-MCP.
 * Provides exported functions for host to execute Perl code and retrieve output.
 * TODO: Integrate actual Perl interpreter (requires perl cross-compiled to WASM)
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

static int perl_initialized = 0;

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
 * TODO: Implement Perl execution
 * Perl can be cross-compiled to WASM using:
 * 1. perl-wasm project (https://github.com/nicj/perl-wasm)
 * 2. Cross-compile Perl 5 with WASI SDK
 * 3. Use a minimal Perl-like interpreter
 *
 * The Perl interpreter needs to be initialized with perl_alloc(), perl_construct()
 * and code executed via perl_eval_sv() or similar.
 */

int perl_wasi_init(void) {
    if (perl_initialized) return 0;

    /* TODO: Initialize Perl interpreter
     * PerlInterpreter *my_perl = perl_alloc();
     * perl_construct(my_perl);
     */

    perl_initialized = 1;
    output_reset();
    return 0;
}

int perl_wasi_exec(const char *src, size_t len) {
    if (!perl_initialized) return -1;

    output_reset();

    /* TODO: Execute Perl source
     * SV *result = perl_eval_sv(my_perl, src, G_SCALAR);
     * const char *output = SvPV_nolen(result);
     * output_append(output, strlen(output));
     */

    output_append("# Perl source: ", 14);
    output_append(src, len);

    return 0;
}

const char *perl_wasi_get_output(void) {
    return output_buf;
}

size_t perl_wasi_get_output_len(void) {
    return output_len;
}

int perl_wasi_set_args(const char *ptr, size_t len) {
    if (len >= ARGS_BUF_SIZE) len = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, ptr, len);
    args_buf[len] = '\0';
    args_len = len;
    return 0;
}

int perl_wasi_register_tool(const char *name_ptr, size_t name_len,
                            const char *src_ptr, size_t src_len) {
    (void)name_ptr; (void)name_len;
    (void)src_ptr; (void)src_len;
    return 0;
}

int perl_wasi_call_tool(const char *args_ptr, size_t args_n,
                        const char *name_ptr, size_t name_len) {
    if (!perl_initialized) return -1;

    if (args_n >= ARGS_BUF_SIZE) args_n = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, args_ptr, args_n);
    args_buf[args_n] = '\0';

    output_reset();

    (void)name_ptr; (void)name_len;

    return 0;
}

void perl_wasi_destroy(void) {
    if (perl_initialized) {
        /* TODO: perl_destruct(my_perl); perl_free(my_perl); */
        perl_initialized = 0;
    }
}
