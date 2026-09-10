/*
 * mruby WASI reactor — minimal Ruby tool execution via mruby.
 *
 * Compiled with: wasi-sdk clang -Wl,--no-entry -Wl,--export-all
 * Provides: mruby_init, mruby_eval, mruby_get_output, mruby_get_output_len,
 *           mruby_register_tool, mruby_call_tool, mruby_destroy
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mruby.h>
#include <mruby/compile.h>
#include <mruby/string.h>
#include <mruby/array.h>
#include <mruby/hash.h>

static mrb_state *mrb = NULL;
static char tool_source[65536] = {0};
static char tool_name[256] = {0};
static char output_buf[65536] = {0};
static char call_tool_ruby_args[32768] = {0};
static char call_tool_code[65536] = {0};

/* Convert JSON text to a Ruby hash literal string.
   Handles: objects, arrays, strings, numbers, true, false, null.
   Output uses Ruby syntax: => for key-value, nil for null. */
static int json_to_ruby_literal(const char *json, int json_len, char *out, int out_cap) {
    int si = 0, di = 0;
    int in_string = 0;
    int is_key = 0;

    while (si < json_len && di < out_cap - 16) {
        char c = json[si];

        if (in_string) {
            out[di++] = c;
            if (c == '\\' && si + 1 < json_len) {
                si++;
                out[di++] = json[si];
            } else if (c == '"') {
                in_string = 0;
            }
            si++;
            continue;
        }

        switch (c) {
        case '"':
            in_string = 1;
            is_key = 1;
            out[di++] = c;
            break;
        case ':':
            if (is_key) {
                out[di++] = '=';
                out[di++] = '>';
                is_key = 0;
            } else {
                out[di++] = c;
            }
            break;
        case ',':
            out[di++] = c;
            is_key = 0;
            break;
        case '{':
        case '[':
            out[di++] = c;
            is_key = 0;
            break;
        case '}':
        case ']':
            out[di++] = c;
            is_key = 0;
            break;
        case 'n':
            if (si + 3 < json_len && memcmp(json + si, "null", 4) == 0) {
                memcpy(out + di, "nil", 3);
                di += 3;
                si += 4;
                continue;
            }
            out[di++] = c;
            break;
        case ' ':
        case '\t':
        case '\n':
        case '\r':
            break;
        default:
            out[di++] = c;
            break;
        }
        si++;
    }
    out[di] = '\0';
    return di;
}

/* Ruby helper that converts a Ruby value to a JSON string using only builtins.
   NOTE: This is kept for potential direct use, but mruby_call_tool now uses
   C-side JSON conversion to avoid block/iterator issues in WASM EH. */
static const char *TO_JSON_HELPER =
    "def __mruby_to_json(obj)\n"
    "  case obj\n"
    "  when Hash\n"
    "    '{' + obj.map{|k,v| k.to_s.inspect + ':' + __mruby_to_json(v)}.join(',') + '}'\n"
    "  when Array\n"
    "    '[' + obj.map{|v| __mruby_to_json(v)}.join(',') + ']'\n"
    "  when String\n"
    "    obj.to_s.inspect\n"
    "  when Integer, Float\n"
    "    obj.to_s\n"
    "  when TrueClass\n"
    "    'true'\n"
    "  when FalseClass\n"
    "    'false'\n"
    "  when NilClass\n"
    "    'null'\n"
    "  else\n"
    "    obj.to_s.inspect\n"
    "  end\n"
    "end\n";

void mruby_init(void) {
    if (mrb) return;
    mrb = mrb_open();
    if (!mrb) {
        fprintf(stderr, "mruby_open() failed\n");
        return;
    }
    mrb_load_nstring(mrb, TO_JSON_HELPER, strlen(TO_JSON_HELPER));
    if (mrb->exc) {
        fprintf(stderr, "Failed to define __mruby_to_json helper\n");
        mrb->exc = NULL;
    }
}

/* --- JSON-to-mrb_value parser: builds mruby objects directly in C, no Ruby parsing --- */

static void json_skip_ws(const char *j, int len, int *p) {
    while (*p < len) {
        char c = j[*p];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') (*p)++;
        else break;
    }
}

static mrb_value json_parse_value(mrb_state *mrb, const char *json, int len, int *pos);

static mrb_value json_parse_string(mrb_state *mrb, const char *json, int len, int *pos) {
    (*pos)++; /* skip opening " */
    char buf[16384];
    int bi = 0;
    while (*pos < len && bi < (int)sizeof(buf) - 4) {
        char c = json[*pos];
        if (c == '"') { (*pos)++; break; }
        if (c == '\\' && *pos + 1 < len) {
            (*pos)++;
            switch (json[*pos]) {
            case '"':  buf[bi++] = '"'; break;
            case '\\': buf[bi++] = '\\'; break;
            case '/':  buf[bi++] = '/'; break;
            case 'b':  buf[bi++] = '\b'; break;
            case 'f':  buf[bi++] = '\f'; break;
            case 'n':  buf[bi++] = '\n'; break;
            case 'r':  buf[bi++] = '\r'; break;
            case 't':  buf[bi++] = '\t'; break;
            case 'u': {
                if (*pos + 4 < len) {
                    char hex[5] = {json[*pos+1], json[*pos+2], json[*pos+3], json[*pos+4], 0};
                    unsigned int cp = (unsigned int)strtoul(hex, NULL, 16);
                    if (cp < 0x80) { buf[bi++] = (char)cp; }
                    else if (cp < 0x800) { buf[bi++] = (char)(0xC0|(cp>>6)); buf[bi++] = (char)(0x80|(cp&0x3F)); }
                    else { buf[bi++] = (char)(0xE0|(cp>>12)); buf[bi++] = (char)(0x80|((cp>>6)&0x3F)); buf[bi++] = (char)(0x80|(cp&0x3F)); }
                    *pos += 4;
                }
                break;
            }
            default: buf[bi++] = json[*pos]; break;
            }
        } else {
            buf[bi++] = c;
        }
        (*pos)++;
    }
    return mrb_str_new(mrb, buf, bi);
}

static mrb_value json_parse_object(mrb_state *mrb, const char *json, int len, int *pos) {
    (*pos)++; /* skip { */
    mrb_value hash = mrb_hash_new(mrb);
    int arena = mrb_gc_arena_save(mrb);
    mrb_gc_protect(mrb, hash);
    json_skip_ws(json, len, pos);
    if (*pos < len && json[*pos] == '}') { (*pos)++; return hash; }
    while (*pos < len) {
        json_skip_ws(json, len, pos);
        if (*pos >= len || json[*pos] != '"') break;
        mrb_value key = json_parse_string(mrb, json, len, pos);
        mrb_gc_protect(mrb, key);
        json_skip_ws(json, len, pos);
        if (*pos < len && json[*pos] == ':') (*pos)++;
        json_skip_ws(json, len, pos);
        mrb_value val = json_parse_value(mrb, json, len, pos);
        mrb_gc_protect(mrb, val);
        mrb_hash_set(mrb, hash, key, val);
        mrb_gc_arena_restore(mrb, arena);
        mrb_gc_protect(mrb, hash);
        json_skip_ws(json, len, pos);
        if (*pos < len && json[*pos] == ',') { (*pos)++; continue; }
        break;
    }
    if (*pos < len && json[*pos] == '}') (*pos)++;
    return hash;
}

static mrb_value json_parse_array(mrb_state *mrb, const char *json, int len, int *pos) {
    (*pos)++; /* skip [ */
    mrb_value arr = mrb_ary_new(mrb);
    int arena = mrb_gc_arena_save(mrb);
    mrb_gc_protect(mrb, arr);
    json_skip_ws(json, len, pos);
    if (*pos < len && json[*pos] == ']') { (*pos)++; return arr; }
    while (*pos < len) {
        json_skip_ws(json, len, pos);
        mrb_value val = json_parse_value(mrb, json, len, pos);
        mrb_gc_protect(mrb, val);
        mrb_ary_push(mrb, arr, val);
        mrb_gc_arena_restore(mrb, arena);
        mrb_gc_protect(mrb, arr);
        json_skip_ws(json, len, pos);
        if (*pos < len && json[*pos] == ',') { (*pos)++; continue; }
        break;
    }
    if (*pos < len && json[*pos] == ']') (*pos)++;
    return arr;
}

static mrb_value json_parse_value(mrb_state *mrb, const char *json, int len, int *pos) {
    json_skip_ws(json, len, pos);
    if (*pos >= len) return mrb_nil_value();
    char c = json[*pos];
    if (c == '"') return json_parse_string(mrb, json, len, pos);
    if (c == '{') return json_parse_object(mrb, json, len, pos);
    if (c == '[') return json_parse_array(mrb, json, len, pos);
    if (c == 't' && *pos + 3 < len && memcmp(json + *pos, "true", 4) == 0) { *pos += 4; return mrb_true_value(); }
    if (c == 'f' && *pos + 4 < len && memcmp(json + *pos, "false", 5) == 0) { *pos += 5; return mrb_false_value(); }
    if (c == 'n' && *pos + 3 < len && memcmp(json + *pos, "null", 4) == 0) { *pos += 4; return mrb_nil_value(); }
    /* number */
    char *end = NULL;
    const char *start = json + *pos;
    int is_float = 0;
    for (int i = *pos; i < len; i++) {
        char ch = json[i];
        if (ch == '.' || ch == 'e' || ch == 'E') is_float = 1;
        if (ch == ',' || ch == '}' || ch == ']' || ch == ' ' || ch == '\n') break;
    }
    if (is_float) {
        double d = strtod(start, &end);
        *pos += (int)(end - start);
        return mrb_float_value(mrb, d);
    } else {
        long long ll = strtoll(start, &end, 10);
        *pos += (int)(end - start);
        return mrb_fixnum_value((mrb_int)ll);
    }
}

/* C-side recursive JSON converter — avoids Ruby blocks which crash under WASM EH. */
static int value_to_json(mrb_state *mrb, mrb_value val, char *out, int out_cap);

static int escape_json_string(const char *src, int src_len, char *out, int out_cap) {
    int di = 0;
    if (di >= out_cap - 2) { out[di] = '\0'; return di; }
    out[di++] = '"';
    for (int si = 0; si < src_len && di < out_cap - 8; si++) {
        unsigned char c = (unsigned char)src[si];
        switch (c) {
        case '"':  out[di++] = '\\'; out[di++] = '"'; break;
        case '\\': out[di++] = '\\'; out[di++] = '\\'; break;
        case '\b': out[di++] = '\\'; out[di++] = 'b'; break;
        case '\f': out[di++] = '\\'; out[di++] = 'f'; break;
        case '\n': out[di++] = '\\'; out[di++] = 'n'; break;
        case '\r': out[di++] = '\\'; out[di++] = 'r'; break;
        case '\t': out[di++] = '\\'; out[di++] = 't'; break;
        default:
            if (c < 0x20) {
                di += snprintf(out + di, out_cap - di, "\\u%04x", c);
            } else {
                out[di++] = c;
            }
            break;
        }
    }
    if (di >= out_cap - 1) di = out_cap - 1;
    out[di++] = '"';
    out[di] = '\0';
    return di;
}

static int value_to_json(mrb_state *mrb, mrb_value val, char *out, int out_cap) {
    if (out_cap <= 0) return 0;

    switch (mrb_type(val)) {
    case MRB_TT_TRUE:
        if (out_cap > 4) memcpy(out, "true", 4);
        out[4] = '\0';
        return 4;

    case MRB_TT_FALSE:
        if (mrb_nil_p(val)) {
            if (out_cap > 4) memcpy(out, "null", 4);
            out[4] = '\0';
            return 4;
        }
        if (out_cap > 5) memcpy(out, "false", 5);
        out[5] = '\0';
        return 5;

    case MRB_TT_INTEGER: {
        int n = snprintf(out, out_cap, "%lld", (long long)mrb_integer(val));
        return n;
    }

    case MRB_TT_FLOAT: {
        int n = snprintf(out, out_cap, "%.17g", mrb_float(val));
        return n;
    }

    case MRB_TT_STRING: {
        return escape_json_string(RSTRING_PTR(val), RSTRING_LEN(val), out, out_cap);
    }

    case MRB_TT_HASH: {
        int di = 0;
        if (di >= out_cap - 2) { out[di] = '\0'; return di; }
        out[di++] = '{';

        mrb_value keys = mrb_funcall(mrb, val, "keys", 0);
        int klen = (int)RARRAY_LEN(keys);

        for (int i = 0; i < klen && di < out_cap - 32; i++) {
            mrb_value key = mrb_ary_entry(keys, i);
            mrb_value kstr = mrb_funcall(mrb, key, "to_s", 0);
            di += escape_json_string(RSTRING_PTR(kstr), RSTRING_LEN(kstr), out + di, out_cap - di);

            if (di >= out_cap - 2) break;
            out[di++] = ':';

            mrb_value kval = mrb_hash_get(mrb, val, key);
            di += value_to_json(mrb, kval, out + di, out_cap - di);

            if (i < klen - 1 && di < out_cap - 2) {
                out[di++] = ',';
            }
        }

        if (di >= out_cap - 1) di = out_cap - 1;
        out[di++] = '}';
        out[di] = '\0';
        return di;
    }

    case MRB_TT_ARRAY: {
        int di = 0;
        if (di >= out_cap - 2) { out[di] = '\0'; return di; }
        out[di++] = '[';

        int alen = (int)RARRAY_LEN(val);
        for (int i = 0; i < alen && di < out_cap - 16; i++) {
            mrb_value elem = mrb_ary_entry(val, i);
            di += value_to_json(mrb, elem, out + di, out_cap - di);
            if (i < alen - 1 && di < out_cap - 2) {
                out[di++] = ',';
            }
        }

        if (di >= out_cap - 1) di = out_cap - 1;
        out[di++] = ']';
        out[di] = '\0';
        return di;
    }

    case MRB_TT_SYMBOL: {
        mrb_value s = mrb_funcall(mrb, val, "to_s", 0);
        return escape_json_string(RSTRING_PTR(s), RSTRING_LEN(s), out, out_cap);
    }

    default: {
        mrb_value s = mrb_funcall(mrb, val, "inspect", 0);
        if (mrb_string_p(s)) {
            return escape_json_string(RSTRING_PTR(s), RSTRING_LEN(s), out, out_cap);
        }
        if (out_cap > 4) memcpy(out, "null", 4);
        out[4] = '\0';
        return 4;
    }
    }
}

int mruby_eval(const char *code, int code_len) {
    if (!mrb) mruby_init();
    if (!mrb) return -1;

    mrb_value result = mrb_load_nstring(mrb, code, code_len);

    if (mrb->exc) {
        mrb_value exc = mrb_obj_value(mrb->exc);
        mrb_value msg = mrb_funcall(mrb, exc, "message", 0);
        if (mrb_string_p(msg)) {
            snprintf(output_buf, sizeof(output_buf), "ERROR: %s", RSTRING_PTR(msg));
        } else {
            snprintf(output_buf, sizeof(output_buf), "ERROR: Ruby exception");
        }
        mrb->exc = NULL;
        return -1;
    }

    mrb_value str = mrb_funcall(mrb, result, "to_s", 0);
    if (mrb_string_p(str)) {
        const char *ptr = RSTRING_PTR(str);
        int len = RSTRING_LEN(str);
        if (len >= (int)sizeof(output_buf)) len = sizeof(output_buf) - 1;
        memcpy(output_buf, ptr, len);
        output_buf[len] = '\0';
    } else {
        output_buf[0] = '\0';
    }

    return 0;
}

const char* mruby_get_output(void) {
    return output_buf;
}

int mruby_get_output_len(void) {
    return strlen(output_buf);
}

int mruby_register_tool(const char *name, int name_len, const char *source, int source_len) {
    if (!mrb) mruby_init();
    if (!mrb) return -1;

    if (name_len >= sizeof(tool_name)) name_len = sizeof(tool_name) - 1;
    if (source_len >= sizeof(tool_source)) source_len = sizeof(tool_source) - 1;

    memcpy(tool_name, name, name_len);
    tool_name[name_len] = '\0';

    memcpy(tool_source, source, source_len);
    tool_source[source_len] = '\0';

    if (mruby_eval(tool_source, source_len) != 0) {
        return -1;
    }

    return 0;
}

int mruby_call_tool(const char *name, int name_len, const char *args_json, int args_len) {
    if (!mrb) {
        snprintf(output_buf, sizeof(output_buf), "ERROR: mruby not initialized");
        return -1;
    }

    /* NUL-terminate the name for mrb_funcall */
    char call_name[256];
    if (name_len >= (int)sizeof(call_name)) name_len = (int)sizeof(call_name) - 1;
    memcpy(call_name, name, name_len);
    call_name[name_len] = '\0';

    /* Parse JSON directly to mruby values — no Ruby parsing */
    int pos = 0;
    mrb_value args = json_parse_value(mrb, args_json, args_len, &pos);

    if (mrb->exc) {
        mrb_value exc = mrb_obj_value(mrb->exc);
        mrb_value msg = mrb_funcall(mrb, exc, "message", 0);
        if (mrb_string_p(msg)) {
            snprintf(output_buf, sizeof(output_buf), "ERROR: %s", RSTRING_PTR(msg));
        } else {
            snprintf(output_buf, sizeof(output_buf), "ERROR: Ruby exception");
        }
        mrb->exc = NULL;
        return -1;
    }

    /* Call the function directly — no Ruby parsing */
    mrb_value result = mrb_funcall(mrb, mrb_top_self(mrb), call_name, 1, args);

    if (mrb->exc) {
        mrb_value exc = mrb_obj_value(mrb->exc);
        mrb_value msg = mrb_funcall(mrb, exc, "message", 0);
        if (mrb_string_p(msg)) {
            snprintf(output_buf, sizeof(output_buf), "ERROR: %s", RSTRING_PTR(msg));
        } else {
            snprintf(output_buf, sizeof(output_buf), "ERROR: Ruby exception");
        }
        mrb->exc = NULL;
        return -1;
    }

    value_to_json(mrb, result, output_buf, sizeof(output_buf));
    return 0;
}

void mruby_destroy(void) {
    if (mrb) {
        mrb_close(mrb);
        mrb = NULL;
    }
}
