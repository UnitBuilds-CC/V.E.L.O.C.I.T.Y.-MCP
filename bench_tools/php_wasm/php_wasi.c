/*
 * PHP WASI reactor wrapper for VELOCITY-MCP.
 * Provides exported functions for host to execute PHP code and retrieve output.
 * Note: This is a template - actual PHP embedding requires php-src build.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Output buffer - fixed size in WASM linear memory */
#define OUTPUT_BUF_SIZE (256 * 1024)
static char output_buf[OUTPUT_BUF_SIZE];
static size_t output_len = 0;

/* Args slot for tool protocol */
#define ARGS_BUF_SIZE 4096
static char args_buf[ARGS_BUF_SIZE];
static size_t args_len = 0;

/* PHP interpreter state (placeholder - requires actual php embed SAPI) */
static int php_initialized = 0;

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
 * TODO: Implement actual PHP embedding
 * - Include php_embed.h from php-src embed SAPI
 * - Initialize PHP embed SAPI in php_wasi_init()
 * - Use zend_eval_string() for execution
 * - Implement JSON encode/decode using php's json_encode/json_decode
 */

/* Initialize PHP runtime */
int php_wasi_init(void) {
    if (php_initialized) {
        return 0;
    }
    
    /* TODO: php_embed_init(0, NULL); */
    
    php_initialized = 1;
    output_reset();
    return 0;
}

/* Execute PHP source code */
int php_wasi_exec(const char *src, size_t len) {
    if (!php_initialized) {
        return -1;
    }
    
    output_reset();
    
    /* TODO: zend_eval_stringl(src, len, "input", 1); */
    
    /* Placeholder: just echo the source */
    output_append("<?php ", 6);
    output_append(src, len);
    
    return 0;
}

/* Get pointer to output buffer */
const char *php_wasi_get_output(void) {
    return output_buf;
}

/* Get output length */
size_t php_wasi_get_output_len(void) {
    return output_len;
}

/* Set tool args for the protocol */
int php_wasi_set_args(const char *ptr, size_t len) {
    if (len >= ARGS_BUF_SIZE) len = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, ptr, len);
    args_buf[len] = '\0';
    args_len = len;
    return 0;
}

/* Register a tool wrapper */
int php_wasi_register_tool(const char *name_ptr, size_t name_len,
                           const char *src_ptr, size_t src_len) {
    /* TODO: Store compiled wrapper for later execution */
    (void)name_ptr;
    (void)name_len;
    (void)src_ptr;
    (void)src_len;
    return 0;
}

/* Call a registered tool */
int php_wasi_call_tool(const char *args_ptr, size_t args_n,
                       const char *name_ptr, size_t name_len) {
    if (!php_initialized) return -1;
    
    /* Copy args */
    if (args_n >= ARGS_BUF_SIZE) args_n = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, args_ptr, args_n);
    args_buf[args_n] = '\0';
    args_len = args_n;
    
    output_reset();
    
    /* TODO: Look up tool by name, execute wrapper */
    (void)name_ptr;
    (void)name_len;
    
    return 0;
}

/* Destroy PHP runtime */
void php_wasi_destroy(void) {
    if (php_initialized) {
        /* TODO: php_embed_shutdown(); */
        php_initialized = 0;
    }
}
