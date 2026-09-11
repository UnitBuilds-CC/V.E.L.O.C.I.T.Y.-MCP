/*
 * Minimal PHP interpreter for WASI reactor.
 * Implements basic PHP-like syntax for tool execution.
 * This is a simplified interpreter, not full PHP.
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
#define TOOL_SRC_SIZE (4 * 1024)
static int php_initialized = 0;
static char tool_source[TOOL_SRC_SIZE];
static size_t tool_source_len = 0;
static char _result[1024];
static int _result_set = 0;

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

/* Simple variable storage */
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

/* Skip whitespace */
static const char* skip_ws(const char *p) {
    while (*p && isspace((unsigned char)*p)) p++;
    return p;
}

/* Parse echo statement */
static int exec_echo(const char **p) {
    *p = skip_ws(*p);
    char buf[1024];
    int i = 0;

    while (**p && **p != ';' && i < sizeof(buf) - 1) {
        if (**p == '$') {
            /* Variable reference */
            (*p)++;
            char varname[64];
            int j = 0;
            while (**p && (isalnum((unsigned char)**p) || **p == '_') && j < sizeof(varname) - 1) {
                varname[j++] = **p;
                (*p)++;
            }
            varname[j] = '\0';
            const char *val = get_var(varname);
            int vlen = strlen(val);
            if (i + vlen < sizeof(buf) - 1) {
                memcpy(buf + i, val, vlen);
                i += vlen;
            }
        } else if (**p == '"' || **p == '\'') {
            /* String literal */
            char quote = **p;
            (*p)++;
            while (**p && **p != quote && i < sizeof(buf) - 1) {
                buf[i++] = **p;
                (*p)++;
            }
            if (**p == quote) (*p)++;
        } else {
            buf[i++] = **p;
            (*p)++;
        }
    }
    buf[i] = '\0';

    if (**p == ';') (*p)++;

    output_append_str(buf);
    return 0;
}

/* Parse assignment: $var = value; */
static int exec_assignment(const char **p, const char *varname) {
    *p = skip_ws(*p);
    if (**p != '=') return -1;
    (*p)++;
    *p = skip_ws(*p);

    char value[256];
    int i = 0;

    while (**p && **p != ';' && i < sizeof(value) - 1) {
        if (**p == '"' || **p == '\'') {
            char quote = **p;
            (*p)++;
            while (**p && **p != quote && i < sizeof(value) - 1) {
                value[i++] = **p;
                (*p)++;
            }
            if (**p == quote) (*p)++;
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

static int exec_return(const char **p) {
    *p += 6;
    *p = skip_ws(*p);
    if (**p == '"') {
        (*p)++;
        char buf[1024];
        int i = 0;
        while (**p && **p != '"' && i < (int)sizeof(buf) - 1) {
            if (**p == '\\' && *(*p + 1) == '"') {
                buf[i++] = '"';
                *p += 2;
            } else if (**p == '$') {
                (*p)++;
                char varname[64];
                int j = 0;
                while (**p && (isalnum((unsigned char)**p) || **p == '_') && j < (int)sizeof(varname) - 1) {
                    varname[j++] = **p;
                    (*p)++;
                }
                varname[j] = '\0';
                const char *val = get_var(varname);
                int vlen = strlen(val);
                if (i + vlen < (int)sizeof(buf) - 1) {
                    memcpy(buf + i, val, vlen);
                    i += vlen;
                }
            } else {
                buf[i++] = **p;
                (*p)++;
            }
        }
        buf[i] = '\0';
        if (**p == '"') (*p)++;
        strncpy(_result, buf, sizeof(_result) - 1);
        _result_set = 1;
    }
    while (**p && **p != ';' && **p != '\n') (*p)++;
    if (**p == ';') (*p)++;
    return 0;
}

/* Execute PHP code */
static int exec_php(const char *src) {
    const char *p = src;

    /* Skip <?php tag if present */
    if (strncmp(p, "<?php", 5) == 0) {
        p += 5;
        p = skip_ws(p);
    }

    while (*p) {
        p = skip_ws(p);
        if (!*p) break;

        if (*p == '$') {
            /* Variable */
            p++;
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
        } else if (strncmp(p, "echo", 4) == 0 && isspace((unsigned char)p[4])) {
            p += 4;
            exec_echo(&p);
        } else if (strncmp(p, "print", 5) == 0 && isspace((unsigned char)p[5])) {
            p += 5;
            exec_echo(&p);
        } else if (strncmp(p, "return", 6) == 0 && isspace((unsigned char)p[6])) {
            exec_return(&p);
        } else if (*p == ';') {
            p++;
        } else {
            p++;
        }
    }

    return 0;
}

int php_wasi_init(void) {
    if (php_initialized) return 0;
    var_count = 0;
    tool_source_len = 0;
    tool_source[0] = '\0';
    _result[0] = '\0';
    _result_set = 0;
    php_initialized = 1;
    output_reset();
    return 0;
}

int php_wasi_exec(const char *src, size_t len) {
    if (!php_initialized) return -1;

    output_reset();

    static char buf[EXEC_SLOT_SIZE];
    if (len >= sizeof(buf)) len = sizeof(buf) - 1;
    memcpy(buf, src, len);
    buf[len] = '\0';

    return exec_php(buf);
}

const char *php_wasi_get_output(void) {
    return output_buf;
}

size_t php_wasi_get_output_len(void) {
    return output_len;
}

int php_wasi_set_args(const char *ptr, size_t len) {
    if (len >= ARGS_BUF_SIZE) len = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, ptr, len);
    args_buf[len] = '\0';
    args_len = len;
    return 0;
}

int php_wasi_register_tool(const char *name_ptr, size_t name_len,
                           const char *src_ptr, size_t src_len) {
    if (src_len >= TOOL_SRC_SIZE) src_len = TOOL_SRC_SIZE - 1;
    memcpy(tool_source, src_ptr, src_len);
    tool_source[src_len] = '\0';
    tool_source_len = src_len;
    (void)name_ptr; (void)name_len;
    return 0;
}

/* Parse JSON args and set as PHP variables */
static void parse_json_args(void) {
    const char *p = args_buf;
    p = skip_ws(p);
    if (*p != '{') return;
    p++;

    while (*p && *p != '}') {
        p = skip_ws(p);
        if (*p == '"') {
            p++;
            char key[64];
            int ki = 0;
            while (*p && *p != '"' && ki < (int)sizeof(key) - 1) {
                if (*p == '\\' && *(p + 1)) {
                    p++;
                }
                key[ki++] = *p++;
            }
            key[ki] = '\0';
            if (*p == '"') p++;

            p = skip_ws(p);
            if (*p == ':') p++;
            p = skip_ws(p);

            char value[256];
            int vi = 0;
            if (*p == '"') {
                p++;
                while (*p && *p != '"' && vi < (int)sizeof(value) - 1) {
                    if (*p == '\\' && *(p + 1)) {
                        p++;
                    }
                    value[vi++] = *p++;
                }
                value[vi] = '\0';
                if (*p == '"') p++;
            } else {
                while (*p && *p != ',' && *p != '}' && vi < (int)sizeof(value) - 1) {
                    value[vi++] = *p++;
                }
                value[vi] = '\0';
                while (vi > 0 && isspace((unsigned char)value[vi - 1])) {
                    value[--vi] = '\0';
                }
            }

            set_var(key, value);

            p = skip_ws(p);
            if (*p == ',') p++;
        } else {
            break;
        }
    }
}

int php_wasi_call_tool(const char *args_ptr, size_t args_n,
                       const char *name_ptr, size_t name_len) {
    if (!php_initialized) return -1;

    if (args_n >= ARGS_BUF_SIZE) args_n = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, args_ptr, args_n);
    args_buf[args_n] = '\0';
    args_len = args_n;

    output_reset();
    _result[0] = '\0';
    _result_set = 0;

    parse_json_args();

    if (tool_source_len > 0) {
        exec_php(tool_source);
    }

    if (_result_set) {
        output_reset();
        char json[2048];
        int jlen = snprintf(json, sizeof(json), "{\"message\":\"%s\"}", _result);
        output_append(json, jlen);
        output_append_str("\n");
    }

    (void)name_ptr; (void)name_len;
    return 0;
}

void php_wasi_destroy(void) {
    if (php_initialized) {
        var_count = 0;
        php_initialized = 0;
    }
}
