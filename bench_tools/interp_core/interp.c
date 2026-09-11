/*
 * Shared Interpreter Core - Implementation
 * Tree-walk interpreter with dialect-driven language frontends.
 * Single compilation unit — included by each frontend .c file.
 */

#include "interp.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <math.h>
#include <stdarg.h>

/* ============================================================================
 * Global State
 * ============================================================================ */

static InterpDialect *g_dialect = NULL;
static int g_initialized = 0;

/* Output buffer */
#define OUTPUT_BUF_SIZE (256 * 1024)
static char output_buf[OUTPUT_BUF_SIZE];
static size_t output_len = 0;

/* Tool result */
static char g_result[4096];
static int g_result_set = 0;

/* Tool source */
#define TOOL_SRC_SIZE (64 * 1024)
static char tool_source[TOOL_SRC_SIZE];
static size_t tool_source_len = 0;

/* Args buffer */
#define ARGS_BUF_SIZE 4096
static char args_buf[ARGS_BUF_SIZE];
static size_t args_len = 0;

/* Error buffer */
static char g_error[INTERP_ERROR_BUF_SIZE];

/* AST Arena */
static char *g_arena = NULL;
static size_t g_arena_size = 0;
static size_t g_arena_used = 0;

/* Global scope */
static Scope *g_global_scope = NULL;
static Scope *g_current_scope = NULL;

/* Evaluator state */
static Value *g_return_value = NULL;
static int g_has_return = 0;
static int g_call_depth = 0;

/* ============================================================================
 * Arena Allocator
 * ============================================================================ */

static void arena_init(void) {
    if (g_arena) return;
    g_arena_size = INTERP_ARENA_SIZE;
    g_arena = (char *)malloc(g_arena_size);
    g_arena_used = 0;
}

static void *arena_alloc(size_t bytes) {
    if (!g_arena) arena_init();
    size_t aligned = (bytes + 7) & ~7;
    if (g_arena_used + aligned > g_arena_size) return NULL;
    void *ptr = g_arena + g_arena_used;
    g_arena_used += aligned;
    memset(ptr, 0, aligned);
    return ptr;
}

static void arena_reset(void) {
    g_arena_used = 0;
}

static void arena_free(void) {
    free(g_arena);
    g_arena = NULL;
    g_arena_size = 0;
    g_arena_used = 0;
}

/* ============================================================================
 * Output Buffer
 * ============================================================================ */

static void output_reset(void) {
    output_len = 0;
    output_buf[0] = '\0';
}

static void output_append(const char *data, size_t len) {
    if (output_len + len >= OUTPUT_BUF_SIZE)
        len = OUTPUT_BUF_SIZE - output_len - 1;
    if (len > 0) {
        memcpy(output_buf + output_len, data, len);
        output_len += len;
        output_buf[output_len] = '\0';
    }
}

static void output_append_str(const char *s) {
    output_append(s, strlen(s));
}

/* ============================================================================
 * Value Operations
 * ============================================================================ */

static Value *value_new(ValType type) {
    Value *v = (Value *)malloc(sizeof(Value));
    if (!v) return NULL;
    memset(v, 0, sizeof(Value));
    v->type = type;
    v->refcount = 1;
    return v;
}

static Value *value_new_null(void) {
    return value_new(VAL_NULL);
}

static Value *value_new_bool(int b) {
    Value *v = value_new(VAL_BOOL);
    if (v) v->as.boolean = b ? 1 : 0;
    return v;
}

static Value *value_new_number(double n) {
    Value *v = value_new(VAL_NUMBER);
    if (v) v->as.number = n;
    return v;
}

static Value *value_new_string(const char *s, size_t len) {
    Value *v = value_new(VAL_STRING);
    if (!v) return NULL;
    v->as.str.data = (char *)malloc(len + 1);
    if (!v->as.str.data) { free(v); return NULL; }
    memcpy(v->as.str.data, s, len);
    v->as.str.data[len] = '\0';
    v->as.str.len = len;
    return v;
}

static Value *value_new_string_cstr(const char *s) {
    return value_new_string(s, strlen(s));
}

static Value *value_new_string_fmt(const char *fmt, ...) {
    char buf[2048];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    if (n < 0) return value_new_string_cstr("");
    return value_new_string(buf, (size_t)n);
}

static Value *value_new_array(void) {
    Value *v = value_new(VAL_ARRAY);
    if (!v) return NULL;
    v->as.arr = (ArrayVal *)malloc(sizeof(ArrayVal));
    if (!v->as.arr) { free(v); return NULL; }
    v->as.arr->items = NULL;
    v->as.arr->len = 0;
    v->as.arr->cap = 0;
    return v;
}

static Value *value_new_map(void) {
    Value *v = value_new(VAL_MAP);
    if (!v) return NULL;
    v->as.map = (MapVal *)malloc(sizeof(MapVal));
    if (!v->as.map) { free(v); return NULL; }
    v->as.map->num_buckets = INTERP_SCOPE_BUCKETS;
    v->as.map->buckets = (MapEntry **)calloc(v->as.map->num_buckets, sizeof(MapEntry *));
    v->as.map->size = 0;
    if (!v->as.map->buckets) { free(v->as.map); free(v); return NULL; }
    return v;
}

static Value *value_new_func(FuncDef *def) {
    Value *v = value_new(VAL_FUNC);
    if (v) v->as.func = def;
    return v;
}

static Value *value_ref(Value *v) {
    if (v) v->refcount++;
    return v;
}

static void value_free(Value *v);

static void value_deref(Value *v) {
    if (!v) return;
    v->refcount--;
    if (v->refcount <= 0) value_free(v);
}

static void value_free(Value *v) {
    if (!v) return;
    switch (v->type) {
    case VAL_STRING:
        free(v->as.str.data);
        break;
    case VAL_ARRAY:
        if (v->as.arr) {
            for (int i = 0; i < v->as.arr->len; i++)
                value_deref(v->as.arr->items[i]);
            free(v->as.arr->items);
            free(v->as.arr);
        }
        break;
    case VAL_MAP:
        if (v->as.map) {
            for (int i = 0; i < v->as.map->num_buckets; i++) {
                MapEntry *e = v->as.map->buckets[i];
                while (e) {
                    MapEntry *next = e->next;
                    free(e->key);
                    value_deref(e->value);
                    free(e);
                    e = next;
                }
            }
            free(v->as.map->buckets);
            free(v->as.map);
        }
        break;
    case VAL_FUNC:
        if (v->as.func) {
            free(v->as.func->name);
            for (int i = 0; i < v->as.func->num_params; i++)
                free(v->as.func->params[i].name);
            free(v->as.func->params);
            free(v->as.func);
        }
        break;
    default:
        break;
    }
    free(v);
}

static int value_is_truthy(Value *v) {
    if (!v) return 0;
    switch (v->type) {
    case VAL_NULL: return 0;
    case VAL_BOOL: return v->as.boolean;
    case VAL_NUMBER: return v->as.number != 0.0;
    case VAL_STRING: return v->as.str.len > 0;
    case VAL_ARRAY: return v->as.arr->len > 0;
    case VAL_MAP: return v->as.map->size > 0;
    case VAL_FUNC: return 1;
    }
    return 0;
}

static char *value_to_string(Value *v, size_t *out_len) {
    if (!v) { *out_len = 0; return strdup(""); }
    switch (v->type) {
    case VAL_NULL:
        *out_len = 4;
        return strdup("null");
    case VAL_BOOL:
        if (v->as.boolean) { *out_len = 4; return strdup("true"); }
        else { *out_len = 5; return strdup("false"); }
    case VAL_NUMBER: {
        char buf[64];
        double n = v->as.number;
        if (n == (double)(long long)n)
            snprintf(buf, sizeof(buf), "%lld", (long long)n);
        else
            snprintf(buf, sizeof(buf), "%.17g", n);
        *out_len = strlen(buf);
        return strdup(buf);
    }
    case VAL_STRING:
        *out_len = v->as.str.len;
        return strndup(v->as.str.data, v->as.str.len);
    case VAL_ARRAY: {
        char buf[2048];
        int pos = 0;
        pos += snprintf(buf + pos, sizeof(buf) - pos, "[");
        for (int i = 0; i < v->as.arr->len && pos < (int)sizeof(buf) - 2; i++) {
            if (i > 0) pos += snprintf(buf + pos, sizeof(buf) - pos, ", ");
            size_t slen;
            char *s = value_to_string(v->as.arr->items[i], &slen);
            if (v->as.arr->items[i]->type == VAL_STRING)
                pos += snprintf(buf + pos, sizeof(buf) - pos, "\"%s\"", s);
            else
                pos += snprintf(buf + pos, sizeof(buf) - pos, "%s", s);
            free(s);
        }
        pos += snprintf(buf + pos, sizeof(buf) - pos, "]");
        *out_len = pos;
        return strdup(buf);
    }
    case VAL_MAP: {
        char buf[2048];
        int pos = 0;
        pos += snprintf(buf + pos, sizeof(buf) - pos, "{");
        int first = 1;
        for (int b = 0; b < v->as.map->num_buckets; b++) {
            for (MapEntry *e = v->as.map->buckets[b]; e; e = e->next) {
                if (!first && pos < (int)sizeof(buf) - 2)
                    pos += snprintf(buf + pos, sizeof(buf) - pos, ", ");
                first = 0;
                size_t slen;
                char *s = value_to_string(e->value, &slen);
                pos += snprintf(buf + pos, sizeof(buf) - pos, "\"%s\": %s", e->key, s);
                free(s);
            }
        }
        pos += snprintf(buf + pos, sizeof(buf) - pos, "}");
        *out_len = pos;
        return strdup(buf);
    }
    case VAL_FUNC:
        *out_len = 10;
        return strdup("<function>");
    }
    *out_len = 0;
    return strdup("");
}

static double value_to_number(Value *v) {
    if (!v) return 0.0;
    switch (v->type) {
    case VAL_NULL: return 0.0;
    case VAL_BOOL: return v->as.boolean ? 1.0 : 0.0;
    case VAL_NUMBER: return v->as.number;
    case VAL_STRING: {
        char *end;
        double n = strtod(v->as.str.data, &end);
        if (end == v->as.str.data) return 0.0;
        return n;
    }
    default: return 0.0;
    }
}

static int value_equals(Value *a, Value *b) {
    if (!a && !b) return 1;
    if (!a || !b) return 0;
    if (a->type != b->type) {
        if (a->type == VAL_NULL || b->type == VAL_NULL) return 0;
        return value_to_number(a) == value_to_number(b);
    }
    switch (a->type) {
    case VAL_NULL: return 1;
    case VAL_BOOL: return a->as.boolean == b->as.boolean;
    case VAL_NUMBER: return a->as.number == b->as.number;
    case VAL_STRING:
        return a->as.str.len == b->as.str.len &&
               memcmp(a->as.str.data, b->as.str.data, a->as.str.len) == 0;
    default: return a == b;
    }
}

/* Array operations */
static void array_push(Value *arr, Value *item) {
    if (!arr || arr->type != VAL_ARRAY) return;
    ArrayVal *a = arr->as.arr;
    if (a->len >= a->cap) {
        int new_cap = a->cap == 0 ? 8 : a->cap * 2;
        a->items = (Value **)realloc(a->items, new_cap * sizeof(Value *));
        a->cap = new_cap;
    }
    a->items[a->len++] = value_ref(item);
}

static Value *array_get(Value *arr, int idx) {
    if (!arr || arr->type != VAL_ARRAY) return NULL;
    if (idx < 0 || idx >= arr->as.arr->len) return NULL;
    return arr->as.arr->items[idx];
}

static void array_set(Value *arr, int idx, Value *val) {
    if (!arr || arr->type != VAL_ARRAY) return;
    if (idx < 0) return;
    ArrayVal *a = arr->as.arr;
    while (idx >= a->cap) {
        int new_cap = a->cap == 0 ? 8 : a->cap * 2;
        a->items = (Value **)realloc(a->items, new_cap * sizeof(Value *));
        for (int i = a->len; i < new_cap; i++) a->items[i] = NULL;
        a->cap = new_cap;
    }
    if (idx >= a->len) {
        for (int i = a->len; i < idx; i++) {
            if (!a->items[i]) a->items[i] = value_new_null();
        }
        a->len = idx + 1;
    }
    value_deref(a->items[idx]);
    a->items[idx] = value_ref(val);
}

/* Map operations */
static unsigned int map_hash(const char *str) {
    unsigned int hash = 5381;
    int c;
    while ((c = *str++))
        hash = ((hash << 5) + hash) + c;
    return hash;
}

static void map_set(Value *m, const char *key, Value *val) {
    if (!m || m->type != VAL_MAP) return;
    MapVal *map = m->as.map;
    unsigned int h = map_hash(key) % map->num_buckets;
    for (MapEntry *e = map->buckets[h]; e; e = e->next) {
        if (strcmp(e->key, key) == 0) {
            value_deref(e->value);
            e->value = value_ref(val);
            return;
        }
    }
    MapEntry *e = (MapEntry *)malloc(sizeof(MapEntry));
    e->key = strdup(key);
    e->value = value_ref(val);
    e->next = map->buckets[h];
    map->buckets[h] = e;
    map->size++;
}

static Value *map_get(Value *m, const char *key) {
    if (!m || m->type != VAL_MAP) return NULL;
    MapVal *map = m->as.map;
    unsigned int h = map_hash(key) % map->num_buckets;
    for (MapEntry *e = map->buckets[h]; e; e = e->next) {
        if (strcmp(e->key, key) == 0) return e->value;
    }
    return NULL;
}

static int map_has(Value *m, const char *key) {
    return map_get(m, key) != NULL;
}

/* ============================================================================
 * Scope Operations
 * ============================================================================ */

static Scope *scope_new(Scope *parent) {
    Scope *s = (Scope *)malloc(sizeof(Scope));
    if (!s) return NULL;
    s->num_buckets = INTERP_SCOPE_BUCKETS;
    s->buckets = (ScopeEntry **)calloc(s->num_buckets, sizeof(ScopeEntry *));
    s->size = 0;
    s->parent = parent;
    return s;
}

static void scope_free(Scope *s) {
    if (!s) return;
    for (int i = 0; i < s->num_buckets; i++) {
        ScopeEntry *e = s->buckets[i];
        while (e) {
            ScopeEntry *next = e->next;
            free(e->name);
            value_deref(e->value);
            free(e);
            e = next;
        }
    }
    free(s->buckets);
    free(s);
}

static Value *scope_get(Scope *s, const char *name) {
    for (Scope *cur = s; cur; cur = cur->parent) {
        unsigned int h = map_hash(name) % cur->num_buckets;
        for (ScopeEntry *e = cur->buckets[h]; e; e = e->next) {
            if (strcmp(e->name, name) == 0) return e->value;
        }
    }
    return NULL;
}

static void scope_set(Scope *s, const char *name, Value *val) {
    unsigned int h = map_hash(name) % s->num_buckets;
    for (ScopeEntry *e = s->buckets[h]; e; e = e->next) {
        if (strcmp(e->name, name) == 0) {
            value_deref(e->value);
            e->value = value_ref(val);
            return;
        }
    }
    ScopeEntry *e = (ScopeEntry *)malloc(sizeof(ScopeEntry));
    e->name = strdup(name);
    e->value = value_ref(val);
    e->next = s->buckets[h];
    s->buckets[h] = e;
    s->size++;
}

static void scope_assign(Scope *s, const char *name, Value *val) {
    for (Scope *cur = s; cur; cur = cur->parent) {
        unsigned int h = map_hash(name) % cur->num_buckets;
        for (ScopeEntry *e = cur->buckets[h]; e; e = e->next) {
            if (strcmp(e->name, name) == 0) {
                value_deref(e->value);
                e->value = value_ref(val);
                return;
            }
        }
    }
    scope_set(s, name, val);
}

/* ============================================================================
 * Tokenizer
 * ============================================================================ */

typedef struct {
    const char *source;
    const char *current;
    const char *end;
    int line;
    int col;
    Token peek;
    int has_peek;
    char error[256];
} Tokenizer;

static void tokenizer_init(Tokenizer *t, const char *src) {
    t->source = src;
    t->current = src;
    t->end = src + strlen(src);
    t->line = 1;
    t->col = 1;
    t->has_peek = 0;
    t->error[0] = '\0';

    if (g_dialect->skip_prefix) {
        size_t plen = strlen(g_dialect->skip_prefix);
        if (strncmp(src, g_dialect->skip_prefix, plen) == 0) {
            t->current += plen;
            t->col += (int)plen;
        }
    }
}

static void skip_whitespace(Tokenizer *t) {
    while (t->current < t->end) {
        char c = *t->current;
        if (c == ' ' || c == '\t' || c == '\r') {
            t->current++;
            t->col++;
        } else if (c == '\n') {
            if (!g_dialect->semicolon_required) {
                break;
            }
            t->current++;
            t->line++;
            t->col = 1;
        } else if (g_dialect->line_comment_hash && c == '#') {
            while (t->current < t->end && *t->current != '\n') t->current++;
        } else if (g_dialect->line_comment_slash && c == '/' && t->current + 1 < t->end && t->current[1] == '/') {
            while (t->current < t->end && *t->current != '\n') t->current++;
        } else if (g_dialect->block_comment && c == '/' && t->current + 1 < t->end && t->current[1] == '*') {
            t->current += 2;
            while (t->current + 1 < t->end) {
                if (*t->current == '*' && t->current[1] == '/') {
                    t->current += 2;
                    break;
                }
                if (*t->current == '\n') { t->line++; t->col = 1; }
                t->current++;
            }
        } else {
            break;
        }
    }
}

static int is_ident_start(char c) {
    return isalpha((unsigned char)c) || c == '_';
}

static int is_ident_char(char c) {
    return isalnum((unsigned char)c) || c == '_';
}

static int keyword_match(const char *pos, const char *word, const char *end) {
    size_t wlen = strlen(word);
    if (pos + wlen > end) return 0;
    if (strncmp(pos, word, wlen) != 0) return 0;
    if (pos + wlen < end && is_ident_char(pos[wlen])) return 0;
    return 1;
}

static int is_print_keyword(const char *pos, const char *end) {
    for (int i = 0; g_dialect->print_keywords[i]; i++) {
        if (keyword_match(pos, g_dialect->print_keywords[i], end))
            return (int)strlen(g_dialect->print_keywords[i]);
    }
    return 0;
}

static int tokenize_one(Tokenizer *t, Token *tok) {
    skip_whitespace(t);

    if (t->current >= t->end) {
        tok->type = TOK_EOF;
        tok->start = t->current;
        tok->len = 0;
        tok->line = t->line;
        tok->col = t->col;
        return 0;
    }

    tok->line = t->line;
    tok->col = t->col;
    tok->start = t->current;

    char c = *t->current;

    if (!g_dialect->semicolon_required && c == '\n') {
        t->current++;
        t->line++;
        t->col = 1;
        tok->type = TOK_NEWLINE;
        tok->len = 1;
        return 0;
    }

    if (isdigit((unsigned char)c) || (c == '.' && t->current + 1 < t->end && isdigit((unsigned char)t->current[1]))) {
        const char *start = t->current;
        while (t->current < t->end && isdigit((unsigned char)*t->current)) t->current++;
        if (t->current < t->end && *t->current == '.') {
            t->current++;
            while (t->current < t->end && isdigit((unsigned char)*t->current)) t->current++;
        }
        if (t->current < t->end && (*t->current == 'e' || *t->current == 'E')) {
            t->current++;
            if (t->current < t->end && (*t->current == '+' || *t->current == '-')) t->current++;
            while (t->current < t->end && isdigit((unsigned char)*t->current)) t->current++;
        }
        tok->type = TOK_NUMBER;
        tok->len = t->current - start;
        tok->num_val = strtod(start, NULL);
        t->col += (int)tok->len;
        return 0;
    }

    if (c == '"' || c == '\'') {
        char quote = c;
        t->current++;
        t->col++;
        const char *start = t->current;
        while (t->current < t->end && *t->current != quote) {
            if (*t->current == '\\' && t->current + 1 < t->end) {
                t->current += 2;
                t->col += 2;
            } else {
                if (*t->current == '\n') { t->line++; t->col = 0; }
                t->current++;
                t->col++;
            }
        }
        tok->type = TOK_STRING;
        tok->start = start;
        tok->len = t->current - start;
        if (t->current < t->end) { t->current++; t->col++; }
        return 0;
    }

    if (g_dialect->var_prefix && c == g_dialect->var_prefix) {
        t->current++;
        t->col++;
        const char *start = t->current;
        while (t->current < t->end && is_ident_char(*t->current)) {
            t->current++;
            t->col++;
        }
        tok->type = TOK_IDENT;
        tok->start = start;
        tok->len = t->current - start;
        return 0;
    }

    if (is_ident_start(c)) {
        const char *start = t->current;
        while (t->current < t->end && is_ident_char(*t->current)) {
            t->current++;
            t->col++;
        }
        size_t len = t->current - start;
        tok->start = start;
        tok->len = len;

        if (len == 4 && strncmp(start, "true", 4) == 0) tok->type = TOK_TRUE;
        else if (len == 5 && strncmp(start, "false", 5) == 0) tok->type = TOK_FALSE;
        else if ((len == 4 && strncmp(start, "null", 4) == 0) ||
                 (len == 4 && strncmp(start, "NULL", 4) == 0) ||
                 (len == 3 && strncmp(start, "nil", 3) == 0)) tok->type = TOK_NULL;
        else if (len == 2 && strncmp(start, "if", 2) == 0) tok->type = TOK_IF;
        else if ((len == 4 && strncmp(start, "else", 4) == 0) ||
                 (len == 5 && strncmp(start, "elseif", 5) == 0) ||
                 (len == 4 && strncmp(start, "elif", 4) == 0)) tok->type = TOK_ELSE;
        else if (len == 5 && strncmp(start, "while", 5) == 0) tok->type = TOK_WHILE;
        else if (len == 3 && strncmp(start, "for", 3) == 0) tok->type = TOK_FOR;
        else if (len == 6 && strncmp(start, "return", 6) == 0) tok->type = TOK_RETURN;
        else if (len == 2 && strncmp(start, "in", 2) == 0) tok->type = TOK_IN;
        else if (g_dialect->func_keyword && keyword_match(start, g_dialect->func_keyword, t->end))
            tok->type = TOK_FUNCTION;
        else if (len == 8 && strncmp(start, "function", 8) == 0) tok->type = TOK_FUNCTION;
        else if (len == 3 && strncmp(start, "and", 3) == 0) tok->type = TOK_AND;
        else if (len == 2 && strncmp(start, "or", 2) == 0) tok->type = TOK_OR;
        else {
            int pklen = is_print_keyword(start, t->end);
            if (pklen > 0) {
                tok->type = TOK_PRINT;
                tok->len = pklen;
                t->current = start + pklen;
                t->col = tok->col + pklen;
            } else {
                tok->type = TOK_IDENT;
            }
        }
        return 0;
    }

    t->current++;
    t->col++;
    tok->len = 1;

    switch (c) {
    case '+': tok->type = TOK_PLUS; break;
    case '-':
        if (t->current < t->end && *t->current == '>') {
            /* not arrow, just minus followed by > */
        }
        tok->type = TOK_MINUS;
        break;
    case '*': tok->type = TOK_STAR; break;
    case '/': tok->type = TOK_SLASH; break;
    case '%': tok->type = TOK_PERCENT; break;
    case '.': tok->type = TOK_DOT; break;
    case '(': tok->type = TOK_LPAREN; break;
    case ')': tok->type = TOK_RPAREN; break;
    case '[': tok->type = TOK_LBRACKET; break;
    case ']': tok->type = TOK_RBRACKET; break;
    case '{': tok->type = TOK_LBRACE; break;
    case '}': tok->type = TOK_RBRACE; break;
    case ',': tok->type = TOK_COMMA; break;
    case ';': tok->type = TOK_SEMICOLON; break;
    case ':': tok->type = TOK_COLON; break;
    case '=':
        if (t->current < t->end && *t->current == '=') {
            t->current++; t->col++; tok->len = 2;
            tok->type = TOK_EQ;
        } else {
            tok->type = TOK_ASSIGN;
        }
        break;
    case '!':
        if (t->current < t->end && *t->current == '=') {
            t->current++; t->col++; tok->len = 2;
        }
        tok->type = TOK_NOT;
        break;
    case '<':
        if (t->current < t->end && *t->current == '=') {
            t->current++; t->col++; tok->len = 2;
            tok->type = TOK_LTE;
        } else if (g_dialect->assign_arrow && t->current < t->end && *t->current == '-') {
            t->current++; t->col++; tok->len = 2;
            tok->type = TOK_ARROW;
        } else {
            tok->type = TOK_LT;
        }
        break;
    case '>':
        if (t->current < t->end && *t->current == '=') {
            t->current++; t->col++; tok->len = 2;
            tok->type = TOK_GTE;
        } else {
            tok->type = TOK_GT;
        }
        break;
    case '&':
        if (t->current < t->end && *t->current == '&') {
            t->current++; t->col++; tok->len = 2;
        }
        tok->type = TOK_AND;
        break;
    case '|':
        if (t->current < t->end && *t->current == '|') {
            t->current++; t->col++; tok->len = 2;
        }
        tok->type = TOK_OR;
        break;
    default:
        snprintf(t->error, sizeof(t->error), "unexpected character '%c' at line %d col %d", c, tok->line, tok->col);
        return -1;
    }
    return 0;
}

static int tok_next(Tokenizer *t, Token *tok) {
    if (t->has_peek) {
        *tok = t->peek;
        t->has_peek = 0;
        return 0;
    }
    return tokenize_one(t, tok);
}

static int tok_peek(Tokenizer *t, Token *tok) {
    if (t->has_peek) {
        *tok = t->peek;
        return 0;
    }
    int rc = tokenize_one(t, &t->peek);
    if (rc == 0) t->has_peek = 1;
    *tok = t->peek;
    return rc;
}

static void skip_newlines(Tokenizer *t) {
    Token peek;
    while (tok_peek(t, &peek) == 0 && peek.type == TOK_NEWLINE) {
        tok_next(t, &peek);
    }
}

static int tok_str_eq(Token *tok, const char *s) {
    size_t slen = strlen(s);
    return tok->len == slen && strncmp(tok->start, s, slen) == 0;
}

static char *tok_to_cstr(Token *tok) {
    char *s = (char *)malloc(tok->len + 1);
    memcpy(s, tok->start, tok->len);
    s[tok->len] = '\0';
    return s;
}

/* ============================================================================
 * Parser
 * ============================================================================ */

typedef struct {
    Tokenizer tokenizer;
    Token current;
    InterpDialect *dialect;
    char error[512];
    int had_error;
} Parser;

static void parser_init(Parser *p, const char *source) {
    tokenizer_init(&p->tokenizer, source);
    p->dialect = g_dialect;
    p->error[0] = '\0';
    p->had_error = 0;
    tok_next(&p->tokenizer, &p->current);
}

static void parser_error(Parser *p, const char *fmt, ...) {
    if (p->had_error) return;
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(p->error, sizeof(p->error), fmt, ap);
    va_end(ap);
    p->had_error = 1;
}

static void advance(Parser *p) {
    Token next;
    if (tok_next(&p->tokenizer, &next) == 0)
        p->current = next;
}

static int check(Parser *p, TokenType type) {
    return p->current.type == type;
}

static int match(Parser *p, TokenType type) {
    if (p->current.type == type) {
        advance(p);
        return 1;
    }
    return 0;
}

static void skip_term(Parser *p) {
    if (p->dialect->semicolon_required) {
        while (check(p, TOK_SEMICOLON)) advance(p);
    } else {
        while (check(p, TOK_NEWLINE) || check(p, TOK_SEMICOLON)) advance(p);
    }
}

static void expect_term(Parser *p) {
    if (p->dialect->semicolon_required) {
        if (check(p, TOK_SEMICOLON)) advance(p);
        else if (!check(p, TOK_RBRACE) && !check(p, TOK_EOF))
            parser_error(p, "expected ';' at line %d col %d", p->current.line, p->current.col);
    } else {
        if (check(p, TOK_NEWLINE) || check(p, TOK_SEMICOLON)) advance(p);
        else if (!check(p, TOK_RBRACE) && !check(p, TOK_EOF))
            parser_error(p, "expected newline or ';' at line %d col %d", p->current.line, p->current.col);
    }
}

static void skip_stmt_newlines(Parser *p) {
    if (!p->dialect->semicolon_required) {
        while (check(p, TOK_NEWLINE)) advance(p);
    }
}

/* Forward declarations */
static AstNode *parse_expr(Parser *p);
static AstNode *parse_statement(Parser *p);

static AstNode *parse_primary(Parser *p) {
    int line = p->current.line, col = p->current.col;

    if (check(p, TOK_NUMBER)) {
        double val = p->current.num_val;
        advance(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_NUMBER_LIT;
        n->line = line; n->col = col;
        n->as.number = val;
        return n;
    }

    if (check(p, TOK_STRING)) {
        char *s = tok_to_cstr(&p->current);
        advance(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_STRING_LIT;
        n->line = line; n->col = col;
        n->as.str = s;
        return n;
    }

    if (check(p, TOK_TRUE)) {
        advance(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_BOOL_LIT;
        n->line = line; n->col = col;
        n->as.boolean = 1;
        return n;
    }

    if (check(p, TOK_FALSE)) {
        advance(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_BOOL_LIT;
        n->line = line; n->col = col;
        n->as.boolean = 0;
        return n;
    }

    if (check(p, TOK_NULL)) {
        advance(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_NULL_LIT;
        n->line = line; n->col = col;
        return n;
    }

    if (check(p, TOK_IDENT)) {
        char *name = tok_to_cstr(&p->current);
        advance(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_IDENT;
        n->line = line; n->col = col;
        n->as.ident = name;
        return n;
    }

    if (match(p, TOK_LPAREN)) {
        AstNode *expr = parse_expr(p);
        if (!match(p, TOK_RPAREN))
            parser_error(p, "expected ')' at line %d", p->current.line);
        return expr;
    }

    if (match(p, TOK_LBRACKET)) {
        AstNode *elements[256];
        int count = 0;
        skip_stmt_newlines(p);
        if (!check(p, TOK_RBRACKET)) {
            elements[count++] = parse_expr(p);
            while (match(p, TOK_COMMA)) {
                skip_stmt_newlines(p);
                if (check(p, TOK_RBRACKET)) break;
                if (count < 256) elements[count++] = parse_expr(p);
            }
        }
        skip_stmt_newlines(p);
        if (!match(p, TOK_RBRACKET))
            parser_error(p, "expected ']' at line %d", p->current.line);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_ARRAY_LIT;
        n->line = line; n->col = col;
        n->as.array_lit.elements = (AstNode **)arena_alloc(count * sizeof(AstNode *));
        memcpy(n->as.array_lit.elements, elements, count * sizeof(AstNode *));
        n->as.array_lit.count = count;
        return n;
    }

    if (match(p, TOK_LBRACE)) {
        AstNode *keys[64], *values[64];
        int count = 0;
        skip_stmt_newlines(p);
        if (!check(p, TOK_RBRACE)) {
            do {
                skip_stmt_newlines(p);
                if (check(p, TOK_RBRACE)) break;
                AstNode *key;
                if (check(p, TOK_IDENT)) {
                    key = parse_primary(p);
                } else if (check(p, TOK_STRING)) {
                    key = parse_primary(p);
                } else {
                    parser_error(p, "expected key in map literal at line %d", p->current.line);
                    break;
                }
                if (!match(p, TOK_COLON)) {
                    parser_error(p, "expected ':' in map literal at line %d", p->current.line);
                    break;
                }
                AstNode *val = parse_expr(p);
                if (count < 64) {
                    keys[count] = key;
                    values[count] = val;
                    count++;
                }
                skip_stmt_newlines(p);
            } while (match(p, TOK_COMMA));
        }
        skip_stmt_newlines(p);
        if (!match(p, TOK_RBRACE))
            parser_error(p, "expected '}' at line %d", p->current.line);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_MAP_LIT;
        n->line = line; n->col = col;
        n->as.map_lit.keys = (AstNode **)arena_alloc(count * sizeof(AstNode *));
        n->as.map_lit.values = (AstNode **)arena_alloc(count * sizeof(AstNode *));
        memcpy(n->as.map_lit.keys, keys, count * sizeof(AstNode *));
        memcpy(n->as.map_lit.values, values, count * sizeof(AstNode *));
        n->as.map_lit.count = count;
        return n;
    }

    if (check(p, TOK_FUNCTION)) {
        advance(p);
        char *name = NULL;
        if (check(p, TOK_IDENT)) {
            name = tok_to_cstr(&p->current);
            advance(p);
        }
        char *params[32];
        int nparams = 0;
        if (match(p, TOK_LPAREN)) {
            if (!check(p, TOK_RPAREN)) {
                do {
                    if (check(p, TOK_IDENT)) {
                        if (nparams < 32) params[nparams++] = tok_to_cstr(&p->current);
                        advance(p);
                    }
                } while (match(p, TOK_COMMA));
            }
            if (!match(p, TOK_RPAREN))
                parser_error(p, "expected ')' after parameters");
        }
        AstNode *body = parse_statement(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_FUNC_DEF;
        n->line = line; n->col = col;
        n->as.func_def.name = name;
        n->as.func_def.params = (char **)arena_alloc(nparams * sizeof(char *));
        memcpy(n->as.func_def.params, params, nparams * sizeof(char *));
        n->as.func_def.num_params = nparams;
        n->as.func_def.body = body;
        return n;
    }

    parser_error(p, "unexpected token at line %d col %d", p->current.line, p->current.col);
    advance(p);
    AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
    n->type = AST_NULL_LIT;
    n->line = line; n->col = col;
    return n;
}

static AstNode *parse_postfix(Parser *p) {
    AstNode *expr = parse_primary(p);

    for (;;) {
        int line = p->current.line, col = p->current.col;

        if (match(p, TOK_LPAREN)) {
            AstNode *args[32];
            int nargs = 0;
            skip_stmt_newlines(p);
            if (!check(p, TOK_RPAREN)) {
                args[nargs++] = parse_expr(p);
                while (match(p, TOK_COMMA)) {
                    skip_stmt_newlines(p);
                    if (nargs < 32) args[nargs++] = parse_expr(p);
                }
            }
            skip_stmt_newlines(p);
            if (!match(p, TOK_RPAREN))
                parser_error(p, "expected ')' after arguments");
            AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
            n->type = AST_CALL;
            n->line = line; n->col = col;
            n->as.call.func = expr;
            n->as.call.args = (AstNode **)arena_alloc(nargs * sizeof(AstNode *));
            memcpy(n->as.call.args, args, nargs * sizeof(AstNode *));
            n->as.call.num_args = nargs;
            expr = n;
        } else if (match(p, TOK_LBRACKET)) {
            AstNode *idx = parse_expr(p);
            if (!match(p, TOK_RBRACKET))
                parser_error(p, "expected ']' after index");
            AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
            n->type = AST_INDEX;
            n->line = line; n->col = col;
            n->as.index.object = expr;
            n->as.index.index = idx;
            expr = n;
        } else if (check(p, TOK_DOT) && !g_dialect->concat_dot) {
            advance(p);
            if (!check(p, TOK_IDENT)) {
                parser_error(p, "expected member name after '.'");
                break;
            }
            char *member = tok_to_cstr(&p->current);
            advance(p);
            AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
            n->type = AST_MEMBER;
            n->line = line; n->col = col;
            n->as.member.object = expr;
            n->as.member.member = member;
            expr = n;
        } else {
            break;
        }
    }
    return expr;
}

static AstNode *parse_unary(Parser *p) {
    int line = p->current.line, col = p->current.col;
    if (match(p, TOK_NOT)) {
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_UNARY;
        n->line = line; n->col = col;
        n->as.unary.op = TOK_NOT;
        n->as.unary.operand = parse_unary(p);
        return n;
    }
    if (match(p, TOK_MINUS)) {
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_UNARY;
        n->line = line; n->col = col;
        n->as.unary.op = TOK_MINUS;
        n->as.unary.operand = parse_unary(p);
        return n;
    }
    return parse_postfix(p);
}

static AstNode *parse_multiplicative(Parser *p) {
    AstNode *left = parse_unary(p);
    while (check(p, TOK_STAR) || check(p, TOK_SLASH) || check(p, TOK_PERCENT)) {
        int op = p->current.type;
        int line = p->current.line, col = p->current.col;
        advance(p);
        AstNode *right = parse_unary(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_BINARY;
        n->line = line; n->col = col;
        n->as.binary.op = op;
        n->as.binary.left = left;
        n->as.binary.right = right;
        left = n;
    }
    return left;
}

static AstNode *parse_additive(Parser *p) {
    AstNode *left = parse_multiplicative(p);
    while (check(p, TOK_PLUS) || check(p, TOK_MINUS) ||
           (g_dialect->concat_dot && check(p, TOK_DOT))) {
        int op = p->current.type;
        int line = p->current.line, col = p->current.col;
        advance(p);
        AstNode *right = parse_multiplicative(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_BINARY;
        n->line = line; n->col = col;
        n->as.binary.op = op;
        n->as.binary.left = left;
        n->as.binary.right = right;
        left = n;
    }
    return left;
}

static AstNode *parse_comparison(Parser *p) {
    AstNode *left = parse_additive(p);
    while (check(p, TOK_LT) || check(p, TOK_GT) || check(p, TOK_LTE) || check(p, TOK_GTE)) {
        int op = p->current.type;
        int line = p->current.line, col = p->current.col;
        advance(p);
        AstNode *right = parse_additive(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_BINARY;
        n->line = line; n->col = col;
        n->as.binary.op = op;
        n->as.binary.left = left;
        n->as.binary.right = right;
        left = n;
    }
    return left;
}

static AstNode *parse_equality(Parser *p) {
    AstNode *left = parse_comparison(p);
    while (check(p, TOK_EQ) || check(p, TOK_NEQ)) {
        int op = p->current.type;
        int line = p->current.line, col = p->current.col;
        advance(p);
        AstNode *right = parse_comparison(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_BINARY;
        n->line = line; n->col = col;
        n->as.binary.op = op;
        n->as.binary.left = left;
        n->as.binary.right = right;
        left = n;
    }
    return left;
}

static AstNode *parse_logical_and(Parser *p) {
    AstNode *left = parse_equality(p);
    while (check(p, TOK_AND)) {
        int line = p->current.line, col = p->current.col;
        advance(p);
        AstNode *right = parse_equality(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_BINARY;
        n->line = line; n->col = col;
        n->as.binary.op = TOK_AND;
        n->as.binary.left = left;
        n->as.binary.right = right;
        left = n;
    }
    return left;
}

static AstNode *parse_logical_or(Parser *p) {
    AstNode *left = parse_logical_and(p);
    while (check(p, TOK_OR)) {
        int line = p->current.line, col = p->current.col;
        advance(p);
        AstNode *right = parse_logical_and(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_BINARY;
        n->line = line; n->col = col;
        n->as.binary.op = TOK_OR;
        n->as.binary.left = left;
        n->as.binary.right = right;
        left = n;
    }
    return left;
}

static AstNode *parse_assignment(Parser *p) {
    AstNode *left = parse_logical_or(p);
    if ((check(p, TOK_ASSIGN) || check(p, TOK_ARROW)) &&
        (left->type == AST_IDENT || left->type == AST_INDEX || left->type == AST_MEMBER)) {
        int line = p->current.line, col = p->current.col;
        advance(p);
        AstNode *value = parse_expr(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_ASSIGN;
        n->line = line; n->col = col;
        n->as.assign.target = left;
        n->as.assign.value = value;
        return n;
    }
    return left;
}

static AstNode *parse_expr(Parser *p) {
    return parse_assignment(p);
}

static AstNode *parse_block(Parser *p) {
    int line = p->current.line, col = p->current.col;
    if (!match(p, TOK_LBRACE)) {
        parser_error(p, "expected '{'");
        return NULL;
    }
    AstNode *stmts[256];
    int count = 0;
    skip_stmt_newlines(p);
    while (!check(p, TOK_RBRACE) && !check(p, TOK_EOF)) {
        AstNode *s = parse_statement(p);
        if (s && count < 256) stmts[count++] = s;
        skip_term(p);
        skip_stmt_newlines(p);
        if (p->had_error) break;
    }
    if (!match(p, TOK_RBRACE))
        parser_error(p, "expected '}'");
    AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
    n->type = AST_BLOCK;
    n->line = line; n->col = col;
    n->as.block.stmts = (AstNode **)arena_alloc(count * sizeof(AstNode *));
    memcpy(n->as.block.stmts, stmts, count * sizeof(AstNode *));
    n->as.block.count = count;
    return n;
}

static AstNode *parse_statement(Parser *p) {
    skip_stmt_newlines(p);
    int line = p->current.line, col = p->current.col;

    if (check(p, TOK_LBRACE)) return parse_block(p);

    if (check(p, TOK_IF)) {
        advance(p);
        int need_paren = check(p, TOK_LPAREN);
        if (need_paren) advance(p);
        AstNode *cond = parse_expr(p);
        if (need_paren) {
            if (!match(p, TOK_RPAREN))
                parser_error(p, "expected ')' after if condition");
        }
        skip_stmt_newlines(p);
        AstNode *then_body = parse_statement(p);
        AstNode *else_body = NULL;
        skip_stmt_newlines(p);
        if (check(p, TOK_ELSE)) {
            advance(p);
            if (check(p, TOK_IF)) {
                else_body = parse_statement(p);
            } else {
                skip_stmt_newlines(p);
                else_body = parse_statement(p);
            }
        }
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_IF;
        n->line = line; n->col = col;
        n->as.if_stmt.cond = cond;
        n->as.if_stmt.then_body = then_body;
        n->as.if_stmt.else_body = else_body;
        return n;
    }

    if (check(p, TOK_WHILE)) {
        advance(p);
        int need_paren = check(p, TOK_LPAREN);
        if (need_paren) advance(p);
        AstNode *cond = parse_expr(p);
        if (need_paren) {
            if (!match(p, TOK_RPAREN))
                parser_error(p, "expected ')' after while condition");
        }
        skip_stmt_newlines(p);
        AstNode *body = parse_statement(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_WHILE;
        n->line = line; n->col = col;
        n->as.while_stmt.cond = cond;
        n->as.while_stmt.body = body;
        return n;
    }

    if (check(p, TOK_FOR)) {
        advance(p);
        int need_paren = check(p, TOK_LPAREN);
        if (need_paren) advance(p);
        skip_stmt_newlines(p);
        AstNode *init = NULL;
        if (!check(p, TOK_SEMICOLON) && !(need_paren == 0 && check(p, TOK_LBRACE))) {
            init = parse_expr(p);
        }
        if (g_dialect->semicolon_required || need_paren) {
            if (!match(p, TOK_SEMICOLON))
                parser_error(p, "expected ';' in for loop");
        } else {
            skip_term(p);
        }
        skip_stmt_newlines(p);
        AstNode *cond = NULL;
        if (!check(p, TOK_SEMICOLON) && !(need_paren == 0 && check(p, TOK_LBRACE))) {
            cond = parse_expr(p);
        }
        if (g_dialect->semicolon_required || need_paren) {
            if (!match(p, TOK_SEMICOLON))
                parser_error(p, "expected ';' in for loop");
        } else {
            skip_term(p);
        }
        skip_stmt_newlines(p);
        AstNode *step = NULL;
        if (!check(p, TOK_RPAREN) && !(need_paren == 0 && check(p, TOK_LBRACE))) {
            step = parse_expr(p);
        }
        if (need_paren) {
            if (!match(p, TOK_RPAREN))
                parser_error(p, "expected ')' after for clauses");
        }
        skip_stmt_newlines(p);
        AstNode *body = parse_statement(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_FOR;
        n->line = line; n->col = col;
        n->as.for_stmt.init = init;
        n->as.for_stmt.cond = cond;
        n->as.for_stmt.step = step;
        n->as.for_stmt.body = body;
        return n;
    }

    if (check(p, TOK_RETURN)) {
        advance(p);
        AstNode *val = NULL;
        if (g_dialect->return_parens && check(p, TOK_LPAREN)) {
            advance(p);
            val = parse_expr(p);
            if (!match(p, TOK_RPAREN))
                parser_error(p, "expected ')' after return value");
        } else if (!check(p, TOK_SEMICOLON) && !check(p, TOK_NEWLINE) &&
                   !check(p, TOK_RBRACE) && !check(p, TOK_EOF)) {
            val = parse_expr(p);
        }
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_RETURN;
        n->line = line; n->col = col;
        n->as.return_stmt.value = val;
        return n;
    }

    if (check(p, TOK_PRINT)) {
        advance(p);
        AstNode *val = parse_expr(p);
        AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
        n->type = AST_PRINT;
        n->line = line; n->col = col;
        n->as.print_stmt.value = val;
        return n;
    }

    if (check(p, TOK_FUNCTION)) {
        AstNode *func = parse_primary(p);
        return func;
    }

    return parse_expr(p);
}

static AstNode *parse_program(Parser *p) {
    int line = 1, col = 1;
    AstNode *stmts[512];
    int count = 0;
    skip_stmt_newlines(p);
    while (!check(p, TOK_EOF) && !p->had_error) {
        AstNode *s = parse_statement(p);
        if (s && count < 512) stmts[count++] = s;
        skip_term(p);
        skip_stmt_newlines(p);
    }
    AstNode *n = (AstNode *)arena_alloc(sizeof(AstNode));
    n->type = AST_BLOCK;
    n->line = line; n->col = col;
    n->as.block.stmts = (AstNode **)arena_alloc(count * sizeof(AstNode *));
    memcpy(n->as.block.stmts, stmts, count * sizeof(AstNode *));
    n->as.block.count = count;
    return n;
}

/* ============================================================================
 * Evaluator
 * ============================================================================ */

static Value *eval_node(AstNode *node, Scope *scope);

static Value *eval_binary(AstNode *node, Scope *scope) {
    Value *left = eval_node(node->as.binary.left, scope);
    if (!left) return value_new_null();

    if (node->as.binary.op == TOK_AND) {
        if (!value_is_truthy(left)) { value_deref(left); return value_new_bool(0); }
        Value *right = eval_node(node->as.binary.right, scope);
        int result = value_is_truthy(right);
        value_deref(left); value_deref(right);
        return value_new_bool(result);
    }
    if (node->as.binary.op == TOK_OR) {
        if (value_is_truthy(left)) return left;
        Value *right = eval_node(node->as.binary.right, scope);
        value_deref(left);
        return right;
    }

    Value *right = eval_node(node->as.binary.right, scope);
    if (!right) { value_deref(left); return value_new_null(); }

    Value *result = NULL;
    int op = node->as.binary.op;

    if (op == TOK_PLUS) {
        if (left->type == VAL_STRING || right->type == VAL_STRING) {
            size_t llen, rlen;
            char *ls = value_to_string(left, &llen);
            char *rs = value_to_string(right, &rlen);
            size_t total = llen + rlen;
            char *buf = (char *)malloc(total + 1);
            memcpy(buf, ls, llen);
            memcpy(buf + llen, rs, rlen);
            buf[total] = '\0';
            result = value_new_string(buf, total);
            free(buf); free(ls); free(rs);
        } else {
            result = value_new_number(value_to_number(left) + value_to_number(right));
        }
    } else if (op == TOK_MINUS) {
        result = value_new_number(value_to_number(left) - value_to_number(right));
    } else if (op == TOK_STAR) {
        result = value_new_number(value_to_number(left) * value_to_number(right));
    } else if (op == TOK_SLASH) {
        double d = value_to_number(right);
        if (d == 0.0) {
            snprintf(g_error, sizeof(g_error), "division by zero at line %d", node->line);
            result = value_new_null();
        } else {
            result = value_new_number(value_to_number(left) / d);
        }
    } else if (op == TOK_PERCENT) {
        double d = value_to_number(right);
        if (d == 0.0) {
            snprintf(g_error, sizeof(g_error), "modulo by zero at line %d", node->line);
            result = value_new_null();
        } else {
            result = value_new_number(fmod(value_to_number(left), d));
        }
    } else if (op == TOK_DOT) {
        size_t llen, rlen;
        char *ls = value_to_string(left, &llen);
        char *rs = value_to_string(right, &rlen);
        size_t total = llen + rlen;
        char *buf = (char *)malloc(total + 1);
        memcpy(buf, ls, llen);
        memcpy(buf + llen, rs, rlen);
        buf[total] = '\0';
        result = value_new_string(buf, total);
        free(buf); free(ls); free(rs);
    } else if (op == TOK_EQ) {
        result = value_new_bool(value_equals(left, right));
    } else if (op == TOK_NEQ) {
        result = value_new_bool(!value_equals(left, right));
    } else if (op == TOK_LT) {
        result = value_new_bool(value_to_number(left) < value_to_number(right));
    } else if (op == TOK_GT) {
        result = value_new_bool(value_to_number(left) > value_to_number(right));
    } else if (op == TOK_LTE) {
        result = value_new_bool(value_to_number(left) <= value_to_number(right));
    } else if (op == TOK_GTE) {
        result = value_new_bool(value_to_number(left) >= value_to_number(right));
    } else {
        result = value_new_null();
    }

    value_deref(left);
    value_deref(right);
    return result;
}

static Value *eval_unary(AstNode *node, Scope *scope) {
    Value *operand = eval_node(node->as.unary.operand, scope);
    if (!operand) return value_new_null();
    Value *result;
    if (node->as.unary.op == TOK_NOT) {
        result = value_new_bool(!value_is_truthy(operand));
    } else if (node->as.unary.op == TOK_MINUS) {
        result = value_new_number(-value_to_number(operand));
    } else {
        result = value_new_null();
    }
    value_deref(operand);
    return result;
}

static Value *eval_call(AstNode *node, Scope *scope);

static Value *eval_node(AstNode *node, Scope *scope) {
    if (!node) return value_new_null();
    if (g_has_return) return value_new_null();

    switch (node->type) {
    case AST_NUMBER_LIT:
        return value_new_number(node->as.number);
    case AST_STRING_LIT:
        return value_new_string_cstr(node->as.str);
    case AST_BOOL_LIT:
        return value_new_bool(node->as.boolean);
    case AST_NULL_LIT:
        return value_new_null();
    case AST_IDENT: {
        Value *v = scope_get(scope, node->as.ident);
        if (!v) {
            snprintf(g_error, sizeof(g_error), "undefined variable '%s' at line %d", node->as.ident, node->line);
            return value_new_null();
        }
        return value_ref(v);
    }
    case AST_BINARY:
        return eval_binary(node, scope);
    case AST_UNARY:
        return eval_unary(node, scope);
    case AST_CALL:
        return eval_call(node, scope);
    case AST_INDEX: {
        Value *obj = eval_node(node->as.index.object, scope);
        Value *idx = eval_node(node->as.index.index, scope);
        Value *result = value_new_null();
        if (obj && obj->type == VAL_ARRAY) {
            int i = (int)value_to_number(idx);
            Value *v = array_get(obj, i);
            if (v) result = value_ref(v);
        } else if (obj && obj->type == VAL_MAP) {
            size_t slen;
            char *key = value_to_string(idx, &slen);
            Value *v = map_get(obj, key);
            if (v) result = value_ref(v);
            free(key);
        }
        value_deref(obj);
        value_deref(idx);
        return result;
    }
    case AST_MEMBER: {
        Value *obj = eval_node(node->as.member.object, scope);
        Value *result = value_new_null();
        if (obj && obj->type == VAL_MAP) {
            Value *v = map_get(obj, node->as.member.member);
            if (v) result = value_ref(v);
        }
        value_deref(obj);
        return result;
    }
    case AST_BLOCK: {
        Scope *block_scope = scope_new(scope);
        Value *last = value_new_null();
        for (int i = 0; i < node->as.block.count; i++) {
            value_deref(last);
            last = eval_node(node->as.block.stmts[i], block_scope);
            if (g_has_return) break;
        }
        scope_free(block_scope);
        return last;
    }
    case AST_ASSIGN: {
        Value *val = eval_node(node->as.assign.value, scope);
        if (node->as.assign.target->type == AST_IDENT) {
            scope_assign(scope, node->as.assign.target->as.ident, val);
        } else if (node->as.assign.target->type == AST_INDEX) {
            AstNode *idx_node = node->as.assign.target;
            Value *obj = eval_node(idx_node->as.index.object, scope);
            Value *idx = eval_node(idx_node->as.index.index, scope);
            if (obj && obj->type == VAL_ARRAY) {
                array_set(obj, (int)value_to_number(idx), val);
            } else if (obj && obj->type == VAL_MAP) {
                size_t slen;
                char *key = value_to_string(idx, &slen);
                map_set(obj, key, val);
                free(key);
            }
            value_deref(obj);
            value_deref(idx);
        } else if (node->as.assign.target->type == AST_MEMBER) {
            AstNode *mem_node = node->as.assign.target;
            Value *obj = eval_node(mem_node->as.member.object, scope);
            if (obj && obj->type == VAL_MAP) {
                map_set(obj, mem_node->as.member.member, val);
            }
            value_deref(obj);
        }
        return val;
    }
    case AST_IF: {
        Value *cond = eval_node(node->as.if_stmt.cond, scope);
        int truthy = value_is_truthy(cond);
        value_deref(cond);
        if (truthy) {
            return eval_node(node->as.if_stmt.then_body, scope);
        } else if (node->as.if_stmt.else_body) {
            return eval_node(node->as.if_stmt.else_body, scope);
        }
        return value_new_null();
    }
    case AST_WHILE: {
        Value *last = value_new_null();
        int iterations = 0;
        while (iterations < 100000) {
            Value *cond = eval_node(node->as.while_stmt.cond, scope);
            if (!value_is_truthy(cond)) { value_deref(cond); break; }
            value_deref(cond);
            value_deref(last);
            last = eval_node(node->as.while_stmt.body, scope);
            if (g_has_return) break;
            iterations++;
        }
        return last;
    }
    case AST_FOR: {
        Scope *for_scope = scope_new(scope);
        if (node->as.for_stmt.init) {
            Value *v = eval_node(node->as.for_stmt.init, for_scope);
            value_deref(v);
        }
        Value *last = value_new_null();
        int iterations = 0;
        while (iterations < 100000) {
            if (node->as.for_stmt.cond) {
                Value *cond = eval_node(node->as.for_stmt.cond, for_scope);
                if (!value_is_truthy(cond)) { value_deref(cond); break; }
                value_deref(cond);
            }
            value_deref(last);
            last = eval_node(node->as.for_stmt.body, for_scope);
            if (g_has_return) break;
            if (node->as.for_stmt.step) {
                Value *v = eval_node(node->as.for_stmt.step, for_scope);
                value_deref(v);
            }
            iterations++;
        }
        scope_free(for_scope);
        return last;
    }
    case AST_RETURN: {
        if (node->as.return_stmt.value) {
            g_return_value = eval_node(node->as.return_stmt.value, scope);
        } else {
            g_return_value = value_new_null();
        }
        g_has_return = 1;
        return value_ref(g_return_value);
    }
    case AST_EXPR_STMT:
        return eval_node(node->as.expr_stmt.expr, scope);
    case AST_FUNC_DEF: {
        FuncDef *def = (FuncDef *)malloc(sizeof(FuncDef));
        def->name = node->as.func_def.name ? strdup(node->as.func_def.name) : NULL;
        def->params = (Param *)malloc(node->as.func_def.num_params * sizeof(Param));
        def->num_params = node->as.func_def.num_params;
        for (int i = 0; i < node->as.func_def.num_params; i++)
            def->params[i].name = strdup(node->as.func_def.params[i]);
        def->body = node->as.func_def.body;
        def->closure = scope;
        Value *v = value_new_func(def);
        if (def->name) {
            scope_set(scope, def->name, v);
        }
        return v;
    }
    case AST_PRINT: {
        Value *val = eval_node(node->as.print_stmt.value, scope);
        size_t slen;
        char *s = value_to_string(val, &slen);
        output_append(s, slen);
        free(s);
        value_deref(val);
        return value_new_null();
    }
    case AST_ARRAY_LIT: {
        Value *arr = value_new_array();
        for (int i = 0; i < node->as.array_lit.count; i++) {
            Value *v = eval_node(node->as.array_lit.elements[i], scope);
            array_push(arr, v);
            value_deref(v);
        }
        return arr;
    }
    case AST_MAP_LIT: {
        Value *m = value_new_map();
        for (int i = 0; i < node->as.map_lit.count; i++) {
            AstNode *key_node = node->as.map_lit.keys[i];
            char *key;
            if (key_node->type == AST_IDENT) {
                key = strdup(key_node->as.ident);
            } else {
                size_t slen;
                key = value_to_string(eval_node(key_node, scope), &slen);
            }
            Value *val = eval_node(node->as.map_lit.values[i], scope);
            map_set(m, key, val);
            free(key);
            value_deref(val);
        }
        return m;
    }
    default:
        return value_new_null();
    }
}

/* ============================================================================
 * Built-in Functions
 * ============================================================================ */

typedef Value *(*BuiltinFn)(Value **args, int nargs);

static Value *builtin_len(Value **args, int nargs) {
    if (nargs < 1) return value_new_number(0);
    if (args[0]->type == VAL_STRING) return value_new_number((double)args[0]->as.str.len);
    if (args[0]->type == VAL_ARRAY) return value_new_number(args[0]->as.arr->len);
    if (args[0]->type == VAL_MAP) return value_new_number(args[0]->as.map->size);
    return value_new_number(0);
}

static Value *builtin_substr(Value **args, int nargs) {
    if (nargs < 2 || args[0]->type != VAL_STRING) return value_new_string_cstr("");
    int start = (int)value_to_number(args[1]);
    int len = nargs >= 3 ? (int)value_to_number(args[2]) : (int)args[0]->as.str.len - start;
    if (start < 0) start = 0;
    if (start > (int)args[0]->as.str.len) start = (int)args[0]->as.str.len;
    if (len < 0) len = 0;
    if (start + len > (int)args[0]->as.str.len) len = (int)args[0]->as.str.len - start;
    return value_new_string(args[0]->as.str.data + start, len);
}

static Value *builtin_split(Value **args, int nargs) {
    if (nargs < 2 || args[0]->type != VAL_STRING || args[1]->type != VAL_STRING)
        return value_new_array();
    Value *arr = value_new_array();
    const char *str = args[0]->as.str.data;
    const char *sep = args[1]->as.str.data;
    size_t sep_len = args[1]->as.str.len;
    if (sep_len == 0) {
        for (size_t i = 0; i < args[0]->as.str.len; i++) {
            Value *ch = value_new_string(str + i, 1);
            array_push(arr, ch);
            value_deref(ch);
        }
        return arr;
    }
    const char *p = str;
    while (p <= str + args[0]->as.str.len) {
        const char *found = strstr(p, sep);
        if (!found) found = str + args[0]->as.str.len;
        Value *part = value_new_string(p, found - p);
        array_push(arr, part);
        value_deref(part);
        p = found + sep_len;
        if (found == str + args[0]->as.str.len) break;
    }
    return arr;
}

static Value *builtin_join(Value **args, int nargs) {
    if (nargs < 2 || args[0]->type != VAL_ARRAY || args[1]->type != VAL_STRING)
        return value_new_string_cstr("");
    Value *arr = args[0];
    const char *sep = args[1]->as.str.data;
    size_t sep_len = args[1]->as.str.len;
    size_t total = 0;
    char **parts = (char **)malloc(arr->as.arr->len * sizeof(char *));
    size_t *part_lens = (size_t *)malloc(arr->as.arr->len * sizeof(size_t));
    for (int i = 0; i < arr->as.arr->len; i++) {
        parts[i] = value_to_string(arr->as.arr->items[i], &part_lens[i]);
        total += part_lens[i];
        if (i > 0) total += sep_len;
    }
    char *buf = (char *)malloc(total + 1);
    size_t pos = 0;
    for (int i = 0; i < arr->as.arr->len; i++) {
        if (i > 0) { memcpy(buf + pos, sep, sep_len); pos += sep_len; }
        memcpy(buf + pos, parts[i], part_lens[i]);
        pos += part_lens[i];
        free(parts[i]);
    }
    buf[total] = '\0';
    free(parts); free(part_lens);
    Value *result = value_new_string(buf, total);
    free(buf);
    return result;
}

static Value *builtin_trim(Value **args, int nargs) {
    if (nargs < 1 || args[0]->type != VAL_STRING) return value_new_string_cstr("");
    const char *s = args[0]->as.str.data;
    size_t len = args[0]->as.str.len;
    while (len > 0 && isspace((unsigned char)s[0])) { s++; len--; }
    while (len > 0 && isspace((unsigned char)s[len - 1])) { len--; }
    return value_new_string(s, len);
}

static Value *builtin_upper(Value **args, int nargs) {
    if (nargs < 1 || args[0]->type != VAL_STRING) return value_new_string_cstr("");
    char *buf = (char *)malloc(args[0]->as.str.len + 1);
    for (size_t i = 0; i < args[0]->as.str.len; i++)
        buf[i] = toupper((unsigned char)args[0]->as.str.data[i]);
    buf[args[0]->as.str.len] = '\0';
    Value *r = value_new_string(buf, args[0]->as.str.len);
    free(buf);
    return r;
}

static Value *builtin_lower(Value **args, int nargs) {
    if (nargs < 1 || args[0]->type != VAL_STRING) return value_new_string_cstr("");
    char *buf = (char *)malloc(args[0]->as.str.len + 1);
    for (size_t i = 0; i < args[0]->as.str.len; i++)
        buf[i] = tolower((unsigned char)args[0]->as.str.data[i]);
    buf[args[0]->as.str.len] = '\0';
    Value *r = value_new_string(buf, args[0]->as.str.len);
    free(buf);
    return r;
}

static Value *builtin_replace(Value **args, int nargs) {
    if (nargs < 3 || args[0]->type != VAL_STRING) return value_new_string_cstr("");
    const char *src = args[0]->as.str.data;
    size_t src_len = args[0]->as.str.len;
    size_t slen, rlen;
    char *search = value_to_string(args[1], &slen);
    char *replace = value_to_string(args[2], &rlen);
    if (slen == 0) { free(search); free(replace); return value_ref(args[0]); }

    char buf[INTERP_MAX_STRING_LEN];
    size_t pos = 0;
    const char *p = src;
    while (p <= src + src_len && pos < sizeof(buf) - 1) {
        const char *found = strstr(p, search);
        if (!found) found = src + src_len;
        size_t chunk = found - p;
        if (pos + chunk >= sizeof(buf) - 1) chunk = sizeof(buf) - 1 - pos;
        memcpy(buf + pos, p, chunk);
        pos += chunk;
        if (found == src + src_len) break;
        if (pos + rlen < sizeof(buf) - 1) {
            memcpy(buf + pos, replace, rlen);
            pos += rlen;
        }
        p = found + slen;
    }
    free(search); free(replace);
    return value_new_string(buf, pos);
}

static Value *builtin_contains(Value **args, int nargs) {
    if (nargs < 2 || args[0]->type != VAL_STRING) return value_new_bool(0);
    size_t slen;
    char *needle = value_to_string(args[1], &slen);
    int found = strstr(args[0]->as.str.data, needle) != NULL;
    free(needle);
    return value_new_bool(found);
}

static Value *builtin_push(Value **args, int nargs) {
    if (nargs < 2 || args[0]->type != VAL_ARRAY) return value_new_null();
    array_push(args[0], args[1]);
    return value_ref(args[0]);
}

static Value *builtin_pop(Value **args, int nargs) {
    if (nargs < 1 || args[0]->type != VAL_ARRAY || args[0]->as.arr->len == 0)
        return value_new_null();
    ArrayVal *a = args[0]->as.arr;
    Value *last = a->items[a->len - 1];
    a->items[a->len - 1] = NULL;
    a->len--;
    return last;
}

static Value *builtin_keys(Value **args, int nargs) {
    if (nargs < 1 || args[0]->type != VAL_MAP) return value_new_array();
    Value *arr = value_new_array();
    MapVal *m = args[0]->as.map;
    for (int b = 0; b < m->num_buckets; b++) {
        for (MapEntry *e = m->buckets[b]; e; e = e->next) {
            Value *k = value_new_string_cstr(e->key);
            array_push(arr, k);
            value_deref(k);
        }
    }
    return arr;
}

static Value *builtin_values(Value **args, int nargs) {
    if (nargs < 1 || args[0]->type != VAL_MAP) return value_new_array();
    Value *arr = value_new_array();
    MapVal *m = args[0]->as.map;
    for (int b = 0; b < m->num_buckets; b++) {
        for (MapEntry *e = m->buckets[b]; e; e = e->next) {
            array_push(arr, e->value);
        }
    }
    return arr;
}

static Value *builtin_has(Value **args, int nargs) {
    if (nargs < 2 || args[0]->type != VAL_MAP) return value_new_bool(0);
    size_t slen;
    char *key = value_to_string(args[1], &slen);
    int found = map_has(args[0], key);
    free(key);
    return value_new_bool(found);
}

static Value *builtin_abs(Value **args, int nargs) {
    if (nargs < 1) return value_new_number(0);
    return value_new_number(fabs(value_to_number(args[0])));
}

static Value *builtin_floor(Value **args, int nargs) {
    if (nargs < 1) return value_new_number(0);
    return value_new_number(floor(value_to_number(args[0])));
}

static Value *builtin_ceil(Value **args, int nargs) {
    if (nargs < 1) return value_new_number(0);
    return value_new_number(ceil(value_to_number(args[0])));
}

static Value *builtin_round(Value **args, int nargs) {
    if (nargs < 1) return value_new_number(0);
    return value_new_number(round(value_to_number(args[0])));
}

static Value *builtin_min(Value **args, int nargs) {
    if (nargs < 1) return value_new_number(0);
    double m = value_to_number(args[0]);
    for (int i = 1; i < nargs; i++) {
        double v = value_to_number(args[i]);
        if (v < m) m = v;
    }
    return value_new_number(m);
}

static Value *builtin_max(Value **args, int nargs) {
    if (nargs < 1) return value_new_number(0);
    double m = value_to_number(args[0]);
    for (int i = 1; i < nargs; i++) {
        double v = value_to_number(args[i]);
        if (v > m) m = v;
    }
    return value_new_number(m);
}

static Value *builtin_type(Value **args, int nargs) {
    if (nargs < 1) return value_new_string_cstr("null");
    switch (args[0]->type) {
    case VAL_NULL: return value_new_string_cstr("null");
    case VAL_BOOL: return value_new_string_cstr("boolean");
    case VAL_NUMBER: return value_new_string_cstr("number");
    case VAL_STRING: return value_new_string_cstr("string");
    case VAL_ARRAY: return value_new_string_cstr("array");
    case VAL_MAP: return value_new_string_cstr("map");
    case VAL_FUNC: return value_new_string_cstr("function");
    }
    return value_new_string_cstr("unknown");
}

static Value *builtin_to_string(Value **args, int nargs) {
    if (nargs < 1) return value_new_string_cstr("");
    size_t slen;
    char *s = value_to_string(args[0], &slen);
    Value *r = value_new_string(s, slen);
    free(s);
    return r;
}

static Value *builtin_to_number(Value **args, int nargs) {
    if (nargs < 1) return value_new_number(0);
    return value_new_number(value_to_number(args[0]));
}

static Value *builtin_set_tool_result(Value **args, int nargs) {
    if (nargs < 1) return value_new_null();
    size_t slen;
    char *s = value_to_string(args[0], &slen);
    if (slen >= sizeof(g_result)) slen = sizeof(g_result) - 1;
    memcpy(g_result, s, slen);
    g_result[slen] = '\0';
    g_result_set = 1;
    free(s);
    return value_new_null();
}

static Value *builtin_map_fn(Value **args, int nargs) {
    if (nargs < 2 || args[0]->type != VAL_ARRAY || args[1]->type != VAL_FUNC)
        return value_new_array();
    Value *arr = args[0];
    FuncDef *fn = args[1]->as.func;
    Value *result = value_new_array();
    for (int i = 0; i < arr->as.arr->len; i++) {
        Scope *call_scope = scope_new(fn->closure ? fn->closure : g_global_scope);
        if (fn->num_params > 0)
            scope_set(call_scope, fn->params[0].name, arr->as.arr->items[i]);
        g_call_depth++;
        Value *v = eval_node(fn->body, call_scope);
        g_call_depth--;
        if (g_has_return) {
            value_deref(v);
            v = g_return_value;
            g_return_value = NULL;
            g_has_return = 0;
        }
        array_push(result, v);
        value_deref(v);
        scope_free(call_scope);
    }
    return result;
}

static Value *builtin_filter(Value **args, int nargs) {
    if (nargs < 2 || args[0]->type != VAL_ARRAY || args[1]->type != VAL_FUNC)
        return value_new_array();
    Value *arr = args[0];
    FuncDef *fn = args[1]->as.func;
    Value *result = value_new_array();
    for (int i = 0; i < arr->as.arr->len; i++) {
        Scope *call_scope = scope_new(fn->closure ? fn->closure : g_global_scope);
        if (fn->num_params > 0)
            scope_set(call_scope, fn->params[0].name, arr->as.arr->items[i]);
        g_call_depth++;
        Value *v = eval_node(fn->body, call_scope);
        g_call_depth--;
        if (g_has_return) {
            value_deref(v);
            v = g_return_value;
            g_return_value = NULL;
            g_has_return = 0;
        }
        if (value_is_truthy(v))
            array_push(result, arr->as.arr->items[i]);
        value_deref(v);
        scope_free(call_scope);
    }
    return result;
}

/* JSON builtins - forward declared, implemented after JSON parser */
static Value *builtin_json_parse(Value **args, int nargs);
static Value *builtin_json_stringify(Value **args, int nargs);

typedef struct {
    const char *name;
    BuiltinFn fn;
} BuiltinEntry;

static BuiltinEntry builtins[] = {
    {"len", builtin_len},
    {"substr", builtin_substr},
    {"split", builtin_split},
    {"join", builtin_join},
    {"trim", builtin_trim},
    {"upper", builtin_upper},
    {"lower", builtin_lower},
    {"replace", builtin_replace},
    {"contains", builtin_contains},
    {"push", builtin_push},
    {"pop", builtin_pop},
    {"keys", builtin_keys},
    {"values", builtin_values},
    {"has", builtin_has},
    {"abs", builtin_abs},
    {"floor", builtin_floor},
    {"ceil", builtin_ceil},
    {"round", builtin_round},
    {"min", builtin_min},
    {"max", builtin_max},
    {"type", builtin_type},
    {"to_string", builtin_to_string},
    {"to_number", builtin_to_number},
    {"set_tool_result", builtin_set_tool_result},
    {"map_fn", builtin_map_fn},
    {"filter", builtin_filter},
    {"json_parse", builtin_json_parse},
    {"json_stringify", builtin_json_stringify},
    {NULL, NULL}
};

static void register_builtins(Scope *scope) {
    for (int i = 0; builtins[i].name; i++) {
        FuncDef *def = (FuncDef *)malloc(sizeof(FuncDef));
        def->name = strdup(builtins[i].name);
        def->params = NULL;
        def->num_params = -1;
        def->body = NULL;
        def->closure = NULL;
        Value *v = value_new_func(def);
        v->as.func = def;
        /* Store builtin pointer in name for dispatch */
        scope_set(scope, builtins[i].name, v);
    }
}

static Value *call_builtin(const char *name, Value **args, int nargs) {
    for (int i = 0; builtins[i].name; i++) {
        if (strcmp(builtins[i].name, name) == 0)
            return builtins[i].fn(args, nargs);
    }
    return NULL;
}

static int is_builtin(const char *name) {
    for (int i = 0; builtins[i].name; i++) {
        if (strcmp(builtins[i].name, name) == 0) return 1;
    }
    return 0;
}

/* ============================================================================
 * JSON Parser & Serializer
 * ============================================================================ */

typedef struct {
    const char *json;
    const char *pos;
    const char *end;
} JsonParser;

static void json_skip_ws(JsonParser *jp) {
    while (jp->pos < jp->end && (*jp->pos == ' ' || *jp->pos == '\t' || *jp->pos == '\n' || *jp->pos == '\r'))
        jp->pos++;
}

static Value *json_parse_value(JsonParser *jp);

static Value *json_parse_string(JsonParser *jp) {
    if (jp->pos >= jp->end || *jp->pos != '"') return value_new_string_cstr("");
    jp->pos++;
    char buf[4096];
    int len = 0;
    while (jp->pos < jp->end && *jp->pos != '"' && len < (int)sizeof(buf) - 4) {
        if (*jp->pos == '\\' && jp->pos + 1 < jp->end) {
            jp->pos++;
            switch (*jp->pos) {
            case '"': buf[len++] = '"'; break;
            case '\\': buf[len++] = '\\'; break;
            case '/': buf[len++] = '/'; break;
            case 'n': buf[len++] = '\n'; break;
            case 't': buf[len++] = '\t'; break;
            case 'r': buf[len++] = '\r'; break;
            case 'b': buf[len++] = '\b'; break;
            case 'f': buf[len++] = '\f'; break;
            default: buf[len++] = *jp->pos; break;
            }
        } else {
            buf[len++] = *jp->pos;
        }
        jp->pos++;
    }
    if (jp->pos < jp->end && *jp->pos == '"') jp->pos++;
    return value_new_string(buf, len);
}

static Value *json_parse_number(JsonParser *jp) {
    const char *start = jp->pos;
    if (jp->pos < jp->end && *jp->pos == '-') jp->pos++;
    while (jp->pos < jp->end && isdigit((unsigned char)*jp->pos)) jp->pos++;
    if (jp->pos < jp->end && *jp->pos == '.') {
        jp->pos++;
        while (jp->pos < jp->end && isdigit((unsigned char)*jp->pos)) jp->pos++;
    }
    if (jp->pos < jp->end && (*jp->pos == 'e' || *jp->pos == 'E')) {
        jp->pos++;
        if (jp->pos < jp->end && (*jp->pos == '+' || *jp->pos == '-')) jp->pos++;
        while (jp->pos < jp->end && isdigit((unsigned char)*jp->pos)) jp->pos++;
    }
    return value_new_number(strtod(start, NULL));
}

static Value *json_parse_object(JsonParser *jp) {
    Value *m = value_new_map();
    jp->pos++; /* skip { */
    json_skip_ws(jp);
    if (jp->pos < jp->end && *jp->pos == '}') { jp->pos++; return m; }
    while (jp->pos < jp->end) {
        json_skip_ws(jp);
        if (jp->pos >= jp->end || *jp->pos != '"') break;
        Value *key = json_parse_string(jp);
        json_skip_ws(jp);
        if (jp->pos < jp->end && *jp->pos == ':') jp->pos++;
        json_skip_ws(jp);
        Value *val = json_parse_value(jp);
        map_set(m, key->as.str.data, val);
        value_deref(key);
        value_deref(val);
        json_skip_ws(jp);
        if (jp->pos < jp->end && *jp->pos == ',') { jp->pos++; continue; }
        break;
    }
    if (jp->pos < jp->end && *jp->pos == '}') jp->pos++;
    return m;
}

static Value *json_parse_array(JsonParser *jp) {
    Value *arr = value_new_array();
    jp->pos++; /* skip [ */
    json_skip_ws(jp);
    if (jp->pos < jp->end && *jp->pos == ']') { jp->pos++; return arr; }
    while (jp->pos < jp->end) {
        json_skip_ws(jp);
        Value *val = json_parse_value(jp);
        array_push(arr, val);
        value_deref(val);
        json_skip_ws(jp);
        if (jp->pos < jp->end && *jp->pos == ',') { jp->pos++; continue; }
        break;
    }
    if (jp->pos < jp->end && *jp->pos == ']') jp->pos++;
    return arr;
}

static Value *json_parse_value(JsonParser *jp) {
    json_skip_ws(jp);
    if (jp->pos >= jp->end) return value_new_null();
    char c = *jp->pos;
    if (c == '"') return json_parse_string(jp);
    if (c == '{') return json_parse_object(jp);
    if (c == '[') return json_parse_array(jp);
    if (c == 't' && jp->pos + 4 <= jp->end && strncmp(jp->pos, "true", 4) == 0) {
        jp->pos += 4; return value_new_bool(1);
    }
    if (c == 'f' && jp->pos + 5 <= jp->end && strncmp(jp->pos, "false", 5) == 0) {
        jp->pos += 5; return value_new_bool(0);
    }
    if (c == 'n' && jp->pos + 4 <= jp->end && strncmp(jp->pos, "null", 4) == 0) {
        jp->pos += 4; return value_new_null();
    }
    if (c == '-' || isdigit((unsigned char)c)) return json_parse_number(jp);
    return value_new_null();
}

static Value *json_parse(const char *json, size_t len) {
    JsonParser jp = { json, json, json + len };
    return json_parse_value(&jp);
}

static void json_serialize_value(Value *v, char *buf, size_t bufsize, size_t *pos) {
    if (!v || v->type == VAL_NULL) {
        *pos += snprintf(buf + *pos, bufsize - *pos, "null");
        return;
    }
    switch (v->type) {
    case VAL_BOOL:
        *pos += snprintf(buf + *pos, bufsize - *pos, v->as.boolean ? "true" : "false");
        break;
    case VAL_NUMBER: {
        double n = v->as.number;
        if (n == (double)(long long)n)
            *pos += snprintf(buf + *pos, bufsize - *pos, "%lld", (long long)n);
        else
            *pos += snprintf(buf + *pos, bufsize - *pos, "%.17g", n);
        break;
    }
    case VAL_STRING: {
        if (*pos < bufsize - 1) buf[(*pos)++] = '"';
        for (size_t i = 0; i < v->as.str.len && *pos < bufsize - 2; i++) {
            char c = v->as.str.data[i];
            switch (c) {
            case '"': buf[(*pos)++] = '\\'; if (*pos < bufsize - 1) buf[(*pos)++] = '"'; break;
            case '\\': buf[(*pos)++] = '\\'; if (*pos < bufsize - 1) buf[(*pos)++] = '\\'; break;
            case '\n': buf[(*pos)++] = '\\'; if (*pos < bufsize - 1) buf[(*pos)++] = 'n'; break;
            case '\t': buf[(*pos)++] = '\\'; if (*pos < bufsize - 1) buf[(*pos)++] = 't'; break;
            case '\r': buf[(*pos)++] = '\\'; if (*pos < bufsize - 1) buf[(*pos)++] = 'r'; break;
            default: buf[(*pos)++] = c; break;
            }
        }
        if (*pos < bufsize - 1) buf[(*pos)++] = '"';
        break;
    }
    case VAL_ARRAY: {
        if (*pos < bufsize - 1) buf[(*pos)++] = '[';
        for (int i = 0; i < v->as.arr->len; i++) {
            if (i > 0 && *pos < bufsize - 1) buf[(*pos)++] = ',';
            json_serialize_value(v->as.arr->items[i], buf, bufsize, pos);
        }
        if (*pos < bufsize - 1) buf[(*pos)++] = ']';
        break;
    }
    case VAL_MAP: {
        if (*pos < bufsize - 1) buf[(*pos)++] = '{';
        int first = 1;
        for (int b = 0; b < v->as.map->num_buckets; b++) {
            for (MapEntry *e = v->as.map->buckets[b]; e; e = e->next) {
                if (!first && *pos < bufsize - 1) buf[(*pos)++] = ',';
                first = 0;
                if (*pos < bufsize - 1) buf[(*pos)++] = '"';
                size_t klen = strlen(e->key);
                for (size_t i = 0; i < klen && *pos < bufsize - 2; i++) {
                    char c = e->key[i];
                    if (c == '"' || c == '\\') buf[(*pos)++] = '\\';
                    buf[(*pos)++] = c;
                }
                if (*pos < bufsize - 1) buf[(*pos)++] = '"';
                if (*pos < bufsize - 1) buf[(*pos)++] = ':';
                json_serialize_value(e->value, buf, bufsize, pos);
            }
        }
        if (*pos < bufsize - 1) buf[(*pos)++] = '}';
        break;
    }
    default:
        *pos += snprintf(buf + *pos, bufsize - *pos, "null");
        break;
    }
}

static char *json_serialize(Value *v, size_t *out_len) {
    char *buf = (char *)malloc(8192);
    size_t pos = 0;
    json_serialize_value(v, buf, 8192, &pos);
    buf[pos] = '\0';
    *out_len = pos;
    return buf;
}

static Value *builtin_json_parse(Value **args, int nargs) {
    if (nargs < 1 || args[0]->type != VAL_STRING) return value_new_null();
    return json_parse(args[0]->as.str.data, args[0]->as.str.len);
}

static Value *builtin_json_stringify(Value **args, int nargs) {
    if (nargs < 1) return value_new_string_cstr("null");
    size_t slen;
    char *s = json_serialize(args[0], &slen);
    Value *r = value_new_string(s, slen);
    free(s);
    return r;
}

/* ============================================================================
 * Function Call Evaluation
 * ============================================================================ */

static Value *eval_call(AstNode *node, Scope *scope) {
    if (g_call_depth >= INTERP_MAX_NEST_DEPTH) {
        snprintf(g_error, sizeof(g_error), "stack overflow (max call depth %d)", INTERP_MAX_NEST_DEPTH);
        return value_new_null();
    }

    /* Get function name if calling an identifier */
    const char *func_name = NULL;
    if (node->as.call.func->type == AST_IDENT)
        func_name = node->as.call.func->as.ident;

    /* Evaluate arguments */
    Value *args[32];
    int nargs = node->as.call.num_args;
    if (nargs > 32) nargs = 32;
    for (int i = 0; i < nargs; i++)
        args[i] = eval_node(node->as.call.args[i], scope);

    /* Check for built-in function */
    if (func_name && is_builtin(func_name)) {
        Value *result = call_builtin(func_name, args, nargs);
        for (int i = 0; i < nargs; i++) value_deref(args[i]);
        return result ? result : value_new_null();
    }

    /* Look up function value */
    Value *func_val = NULL;
    if (func_name) {
        func_val = scope_get(scope, func_name);
    } else {
        func_val = eval_node(node->as.call.func, scope);
    }

    if (!func_val || func_val->type != VAL_FUNC || !func_val->as.func->body) {
        snprintf(g_error, sizeof(g_error), "undefined function '%s' at line %d",
                 func_name ? func_name : "<anonymous>", node->line);
        for (int i = 0; i < nargs; i++) value_deref(args[i]);
        if (!func_name) value_deref(func_val);
        return value_new_null();
    }

    FuncDef *def = func_val->as.func;
    Scope *call_scope = scope_new(def->closure ? def->closure : g_global_scope);

    /* Bind parameters */
    for (int i = 0; i < def->num_params && i < nargs; i++) {
        scope_set(call_scope, def->params[i].name, args[i]);
    }

    g_call_depth++;
    Value *result = eval_node(def->body, call_scope);
    g_call_depth--;

    if (g_has_return) {
        value_deref(result);
        result = g_return_value ? value_ref(g_return_value) : value_new_null();
        g_return_value = NULL;
        g_has_return = 0;
    }

    scope_free(call_scope);
    for (int i = 0; i < nargs; i++) value_deref(args[i]);
    if (!func_name) value_deref(func_val);

    return result;
}

/* ============================================================================
 * Public API
 * ============================================================================ */

void interp_init(InterpDialect *dialect) {
    if (g_initialized) return;
    g_dialect = dialect;
    arena_init();
    g_global_scope = scope_new(NULL);
    g_current_scope = g_global_scope;
    register_builtins(g_global_scope);
    output_reset();
    g_result[0] = '\0';
    g_result_set = 0;
    g_error[0] = '\0';
    g_has_return = 0;
    g_call_depth = 0;
    g_initialized = 1;
}

void interp_reset(void) {
    if (!g_initialized) return;
    /* Free and recreate global scope */
    scope_free(g_global_scope);
    arena_reset();
    g_global_scope = scope_new(NULL);
    g_current_scope = g_global_scope;
    register_builtins(g_global_scope);
    output_reset();
    g_result[0] = '\0';
    g_result_set = 0;
    g_error[0] = '\0';
    g_has_return = 0;
    g_call_depth = 0;
}

void interp_destroy(void) {
    if (!g_initialized) return;
    scope_free(g_global_scope);
    g_global_scope = NULL;
    g_current_scope = NULL;
    arena_free();
    g_initialized = 0;
    g_dialect = NULL;
}

int interp_exec(const char *source) {
    if (!g_initialized) return -1;
    output_reset();
    g_error[0] = '\0';
    g_has_return = 0;

    arena_reset();
    /* Recreate scope after arena reset since AST pointers are invalidated */
    scope_free(g_global_scope);
    g_global_scope = scope_new(NULL);
    g_current_scope = g_global_scope;
    register_builtins(g_global_scope);

    Parser parser;
    parser_init(&parser, source);
    AstNode *program = parse_program(&parser);

    if (parser.had_error) {
        snprintf(g_error, sizeof(g_error), "parse error: %s", parser.error);
        output_append_str(g_error);
        return -1;
    }

    Value *result = eval_node(program, g_current_scope);
    value_deref(result);

    if (g_error[0]) {
        output_append_str(g_error);
        return -1;
    }

    return 0;
}

void interp_register_tool(const char *name_ptr, size_t name_len,
                          const char *src_ptr, size_t src_len) {
    if (src_len >= TOOL_SRC_SIZE) src_len = TOOL_SRC_SIZE - 1;
    memcpy(tool_source, src_ptr, src_len);
    tool_source[src_len] = '\0';
    tool_source_len = src_len;
    (void)name_ptr;
    (void)name_len;
}

int interp_call_tool(const char *args_ptr, size_t args_n,
                     const char *name_ptr, size_t name_len) {
    if (!g_initialized) return -1;

    if (args_n >= ARGS_BUF_SIZE) args_n = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, args_ptr, args_n);
    args_buf[args_n] = '\0';
    args_len = args_n;

    output_reset();
    g_result[0] = '\0';
    g_result_set = 0;

    /* Parse JSON args and bind to scope variables */
    Value *args_map = json_parse(args_buf, args_len);
    if (args_map && args_map->type == VAL_MAP) {
        MapVal *m = args_map->as.map;
        for (int b = 0; b < m->num_buckets; b++) {
            for (MapEntry *e = m->buckets[b]; e; e = e->next) {
                scope_set(g_current_scope, e->key, e->value);
            }
        }
    }
    value_deref(args_map);

    /* Execute tool source */
    if (tool_source_len > 0) {
        g_error[0] = '\0';
        g_has_return = 0;

        Parser parser;
        parser_init(&parser, tool_source);
        AstNode *program = parse_program(&parser);

        if (!parser.had_error) {
            Value *result = eval_node(program, g_current_scope);
            value_deref(result);
        } else {
            snprintf(g_error, sizeof(g_error), "parse error: %s", parser.error);
        }
    }

    /* Format output */
    if (g_result_set) {
        output_reset();
        char json[8192];
        size_t pos = 0;
        pos += snprintf(json + pos, sizeof(json) - pos, "{\"message\":");
        /* Serialize the result value as JSON */
        Value *result_val = value_new_string_cstr(g_result);
        size_t slen;
        char *serialized = json_serialize(result_val, &slen);
        memcpy(json + pos, serialized, slen);
        pos += slen;
        free(serialized);
        value_deref(result_val);
        pos += snprintf(json + pos, sizeof(json) - pos, "}");
        output_append(json, pos);
        output_append_str("\n");
    } else if (g_error[0]) {
        output_reset();
        char json[2048];
        int jlen = snprintf(json, sizeof(json), "{\"error\":\"%s\"}", g_error);
        output_append(json, jlen);
        output_append_str("\n");
    }

    (void)name_ptr;
    (void)name_len;
    return 0;
}

const char *interp_get_output(void) {
    return output_buf;
}

size_t interp_get_output_len(void) {
    return output_len;
}

void interp_set_result(const char *value) {
    strncpy(g_result, value, sizeof(g_result) - 1);
    g_result[sizeof(g_result) - 1] = '\0';
    g_result_set = 1;
}

const char *interp_get_result(void) {
    return g_result_set ? g_result : NULL;
}
