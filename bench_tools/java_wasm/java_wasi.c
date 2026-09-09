/*
 * Minimal Java interpreter for WASI reactor.
 * Implements basic Java-like syntax for tool execution.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>

#define OUTPUT_BUF_SIZE (256 * 1024)
static char output_buf[OUTPUT_BUF_SIZE];
static size_t output_len = 0;

#define ARGS_BUF_SIZE 4096
static char args_buf[ARGS_BUF_SIZE];
static size_t args_len = 0;

#define EXEC_SLOT_SIZE (64 * 1024)
static int java_initialized = 0;

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

static void output_append_str(const char *s) {
    output_append(s, strlen(s));
}

#define MAX_VARS 64
#define VAR_NAME_LEN 64
#define VAR_VALUE_LEN 256

typedef struct {
    char name[VAR_NAME_LEN];
    char value[VAR_VALUE_LEN];
} Variable;

static Variable variables[MAX_VARS];
static int var_count = 0;

static void set_var(const char *name, const char *value) {
    for (int i = 0; i < var_count; i++) {
        if (strcmp(variables[i].name, name) == 0) {
            strncpy(variables[i].value, value, VAR_VALUE_LEN - 1);
            return;
        }
    }
    if (var_count < MAX_VARS) {
        strncpy(variables[var_count].name, name, VAR_NAME_LEN - 1);
        strncpy(variables[var_count].value, value, VAR_VALUE_LEN - 1);
        var_count++;
    }
}

static const char* get_var(const char *name) {
    for (int i = 0; i < var_count; i++) {
        if (strcmp(variables[i].name, name) == 0) {
            return variables[i].value;
        }
    }
    return "";
}

static const char* skip_ws(const char *p) {
    while (*p && isspace((unsigned char)*p)) p++;
    return p;
}

static int exec_system_out_println(const char **p) {
    *p = skip_ws(*p);
    if (**p != '(') return -1;
    (*p)++;

    char buf[1024];
    int i = 0;

    while (**p && **p != ')' && i < sizeof(buf) - 1) {
        if (**p == '"') {
            (*p)++;
            while (**p && **p != '"' && i < sizeof(buf) - 1) {
                if (**p == '\\' && *(*p + 1) == 'n') {
                    buf[i++] = '\n';
                    (*p) += 2;
                } else {
                    buf[i++] = **p;
                    (*p)++;
                }
            }
            if (**p == '"') (*p)++;
        } else {
            buf[i++] = **p;
            (*p)++;
        }
    }
    buf[i] = '\0';

    if (**p == ')') (*p)++;

    output_append_str(buf);
    output_append_str("\n");
    return 0;
}

static int exec_system_out_print(const char **p) {
    *p = skip_ws(*p);
    if (**p != '(') return -1;
    (*p)++;

    char buf[1024];
    int i = 0;

    while (**p && **p != ')' && i < sizeof(buf) - 1) {
        if (**p == '"') {
            (*p)++;
            while (**p && **p != '"' && i < sizeof(buf) - 1) {
                buf[i++] = **p;
                (*p)++;
            }
            if (**p == '"') (*p)++;
        } else {
            buf[i++] = **p;
            (*p)++;
        }
    }
    buf[i] = '\0';

    if (**p == ')') (*p)++;

    output_append_str(buf);
    return 0;
}

static int exec_assignment(const char **p, const char *varname) {
    *p = skip_ws(*p);
    if (**p != '=') return -1;
    (*p)++;
    *p = skip_ws(*p);

    char value[256];
    int i = 0;

    while (**p && **p != ';' && i < sizeof(value) - 1) {
        if (**p == '"') {
            (*p)++;
            while (**p && **p != '"' && i < sizeof(value) - 1) {
                value[i++] = **p;
                (*p)++;
            }
            if (**p == '"') (*p)++;
        } else {
            value[i++] = **p;
            (*p)++;
        }
    }
    value[i] = '\0';

    if (**p == ';') (*p)++;

    set_var(varname, value);
    return 0;
}

static int exec_java(const char *src) {
    const char *p = src;

    while (*p) {
        p = skip_ws(p);
        if (!*p) break;

        if (strncmp(p, "System.out.println", 18) == 0) {
            p += 18;
            exec_system_out_println(&p);
        } else if (strncmp(p, "System.out.print", 16) == 0) {
            p += 16;
            exec_system_out_print(&p);
        } else if (strncmp(p, "String", 6) == 0 && isspace((unsigned char)p[6])) {
            p += 6;
            p = skip_ws(p);
            char varname[64];
            int i = 0;
            while (*p && (isalnum((unsigned char)*p) || *p == '_') && i < sizeof(varname) - 1) {
                varname[i++] = *p;
                p++;
            }
            varname[i] = '\0';
            p = skip_ws(p);
            if (*p == '=') {
                exec_assignment(&p, varname);
            }
        } else if (strncmp(p, "int", 3) == 0 && isspace((unsigned char)p[3])) {
            p += 3;
            p = skip_ws(p);
            char varname[64];
            int i = 0;
            while (*p && (isalnum((unsigned char)*p) || *p == '_') && i < sizeof(varname) - 1) {
                varname[i++] = *p;
                p++;
            }
            varname[i] = '\0';
            p = skip_ws(p);
            if (*p == '=') {
                exec_assignment(&p, varname);
            }
        } else if (*p == ';') {
            p++;
        } else {
            p++;
        }
    }

    return 0;
}

int java_wasi_init(void) {
    if (java_initialized) return 0;
    var_count = 0;
    java_initialized = 1;
    output_reset();
    return 0;
}

int java_wasi_exec(const char *src, size_t len) {
    if (!java_initialized) return -1;

    output_reset();

    char buf[EXEC_SLOT_SIZE];
    if (len >= sizeof(buf)) len = sizeof(buf) - 1;
    memcpy(buf, src, len);
    buf[len] = '\0';

    return exec_java(buf);
}

const char *java_wasi_get_output(void) {
    return output_buf;
}

size_t java_wasi_get_output_len(void) {
    return output_len;
}

int java_wasi_set_args(const char *ptr, size_t len) {
    if (len >= ARGS_BUF_SIZE) len = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, ptr, len);
    args_buf[len] = '\0';
    args_len = len;
    return 0;
}

int java_wasi_register_tool(const char *name_ptr, size_t name_len,
                            const char *src_ptr, size_t src_len) {
    (void)name_ptr; (void)name_len;
    (void)src_ptr; (void)src_len;
    return 0;
}

int java_wasi_call_tool(const char *args_ptr, size_t args_n,
                        const char *name_ptr, size_t name_len) {
    if (!java_initialized) return -1;

    if (args_n >= ARGS_BUF_SIZE) args_n = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, args_ptr, args_n);
    args_buf[args_n] = '\0';

    output_reset();

    (void)name_ptr; (void)name_len;

    return 0;
}

void java_wasi_destroy(void) {
    if (java_initialized) {
        var_count = 0;
        java_initialized = 0;
    }
}
