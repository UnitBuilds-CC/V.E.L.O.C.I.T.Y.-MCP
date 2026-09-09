/*
 * Lua WASI reactor wrapper for VELOCITY-MCP.
 * Provides exported functions for host to execute Lua code and retrieve output.
 * Includes built-in JSON encode/decode for tool argument serialization.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#include "lua.h"
#include "lualib.h"
#include "lauxlib.h"

/* Output buffer - fixed size in WASM linear memory */
#define OUTPUT_BUF_SIZE (256 * 1024)
static char output_buf[OUTPUT_BUF_SIZE];
static size_t output_len = 0;

static lua_State *L = NULL;

/* Reset output buffer */
static void output_reset(void) {
    output_len = 0;
    output_buf[0] = '\0';
}

/* Append to output buffer */
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

/* Custom print function that captures output to buffer */
static int lua_wasi_print(lua_State *ls) {
    int n = lua_gettop(ls);
    if (n > 0) {
        output_append("\n", 1);
    }
    for (int i = 1; i <= n; i++) {
        size_t len;
        const char *s = luaL_tolstring(ls, i, &len);
        if (i > 1) {
            output_append("\t", 1);
        }
        output_append(s, len);
    }
    output_append("\n", 1);
    return 0;
}

/* ---- JSON decoder (recursive descent) ---- */

typedef struct {
    const char *s;
    size_t pos;
    size_t len;
} JsonParser;

static void jp_skip_ws(JsonParser *p) {
    while (p->pos < p->len) {
        char c = p->s[p->pos];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') p->pos++;
        else break;
    }
}

static int jp_parse_value(JsonParser *p, lua_State *ls);

static void jp_parse_string(JsonParser *p, lua_State *ls) {
    /* p->s[p->pos] == '"' */
    p->pos++; /* skip opening quote */
    /* Build string in Lua's buffer via luaL_buffinit */
    luaL_Buffer b;
    luaL_buffinit(ls, &b);
    while (p->pos < p->len && p->s[p->pos] != '"') {
        char c = p->s[p->pos];
        if (c == '\\') {
            p->pos++;
            if (p->pos >= p->len) break;
            c = p->s[p->pos];
            switch (c) {
                case '"': luaL_addchar(&b, '"'); break;
                case '\\': luaL_addchar(&b, '\\'); break;
                case '/': luaL_addchar(&b, '/'); break;
                case 'b': luaL_addchar(&b, '\b'); break;
                case 'f': luaL_addchar(&b, '\f'); break;
                case 'n': luaL_addchar(&b, '\n'); break;
                case 'r': luaL_addchar(&b, '\r'); break;
                case 't': luaL_addchar(&b, '\t'); break;
                case 'u': {
                    /* skip 4 hex digits, emit '?' as placeholder */
                    p->pos += 4;
                    luaL_addchar(&b, '?');
                    break;
                }
                default: luaL_addchar(&b, c); break;
            }
        } else {
            luaL_addchar(&b, c);
        }
        p->pos++;
    }
    if (p->pos < p->len) p->pos++; /* skip closing quote */
    luaL_pushresult(&b);
}

static void jp_parse_number(JsonParser *p, lua_State *ls) {
    size_t start = p->pos;
    int is_float = 0;
    if (p->pos < p->len && p->s[p->pos] == '-') p->pos++;
    while (p->pos < p->len && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++;
    if (p->pos < p->len && p->s[p->pos] == '.') { is_float = 1; p->pos++; while (p->pos < p->len && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++; }
    if (p->pos < p->len && (p->s[p->pos] == 'e' || p->s[p->pos] == 'E')) {
        is_float = 1; p->pos++;
        if (p->pos < p->len && (p->s[p->pos] == '+' || p->s[p->pos] == '-')) p->pos++;
        while (p->pos < p->len && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++;
    }
    char buf[64];
    size_t nlen = p->pos - start;
    if (nlen >= sizeof(buf)) nlen = sizeof(buf) - 1;
    memcpy(buf, p->s + start, nlen);
    buf[nlen] = '\0';
    if (is_float) {
        lua_pushnumber(ls, strtod(buf, NULL));
    } else {
        lua_Integer iv = (lua_Integer)strtoll(buf, NULL, 10);
        lua_pushinteger(ls, iv);
    }
}

static void jp_parse_array(JsonParser *p, lua_State *ls) {
    p->pos++; /* skip '[' */
    lua_newtable(ls);
    jp_skip_ws(p);
    if (p->pos < p->len && p->s[p->pos] == ']') { p->pos++; return; }
    lua_Integer idx = 1;
    for (;;) {
        jp_skip_ws(p);
        jp_parse_value(p, ls);
        lua_rawseti(ls, -2, idx++);
        jp_skip_ws(p);
        if (p->pos < p->len && p->s[p->pos] == ',') { p->pos++; continue; }
        break;
    }
    jp_skip_ws(p);
    if (p->pos < p->len && p->s[p->pos] == ']') p->pos++;
}

static void jp_parse_object(JsonParser *p, lua_State *ls) {
    p->pos++; /* skip '{' */
    lua_newtable(ls);
    jp_skip_ws(p);
    if (p->pos < p->len && p->s[p->pos] == '}') { p->pos++; return; }
    for (;;) {
        jp_skip_ws(p);
        jp_parse_string(p, ls);   /* key on stack */
        jp_skip_ws(p);
        if (p->pos < p->len && p->s[p->pos] == ':') p->pos++;
        jp_skip_ws(p);
        jp_parse_value(p, ls);    /* value on stack */
        lua_rawset(ls, -3);       /* t[key] = value */
        jp_skip_ws(p);
        if (p->pos < p->len && p->s[p->pos] == ',') { p->pos++; continue; }
        break;
    }
    jp_skip_ws(p);
    if (p->pos < p->len && p->s[p->pos] == '}') p->pos++;
}

static int jp_parse_value(JsonParser *p, lua_State *ls) {
    jp_skip_ws(p);
    if (p->pos >= p->len) { lua_pushnil(ls); return 0; }
    char c = p->s[p->pos];
    if (c == '"') { jp_parse_string(p, ls); return 0; }
    if (c == '{') { jp_parse_object(p, ls); return 0; }
    if (c == '[') { jp_parse_array(p, ls); return 0; }
    if (c == 't' && p->pos + 3 < p->len && memcmp(p->s + p->pos, "true", 4) == 0) {
        p->pos += 4; lua_pushboolean(ls, 1); return 0;
    }
    if (c == 'f' && p->pos + 4 < p->len && memcmp(p->s + p->pos, "false", 5) == 0) {
        p->pos += 5; lua_pushboolean(ls, 0); return 0;
    }
    if (c == 'n' && p->pos + 3 < p->len && memcmp(p->s + p->pos, "null", 4) == 0) {
        p->pos += 4; lua_pushnil(ls); return 0;
    }
    if (c == '-' || (c >= '0' && c <= '9')) { jp_parse_number(p, ls); return 0; }
    lua_pushnil(ls);
    return 0;
}

/* json_decode(json_string) -> lua_value */
static int c_json_decode(lua_State *ls) {
    size_t len;
    const char *s = luaL_checklstring(ls, 1, &len);
    JsonParser p = { .s = s, .pos = 0, .len = len };
    jp_parse_value(&p, ls);
    return 1;
}

/* ---- JSON encoder ---- */

static void json_encode_value(lua_State *ls, int idx, luaL_Buffer *b, int depth);

static int is_array_table(lua_State *ls, int idx) {
    lua_Integer n = 1;
    lua_pushvalue(ls, idx);
    lua_pushnil(ls);
    while (lua_next(ls, -2) != 0) {
        /* key at -2, value at -1 */
        if (lua_type(ls, -2) != LUA_TNUMBER) { lua_pop(ls, 2); lua_pop(ls, 1); return 0; }
        if (!lua_isinteger(ls, -2)) { lua_pop(ls, 2); lua_pop(ls, 1); return 0; }
        lua_Integer k = lua_tointeger(ls, -2);
        if (k != n) { lua_pop(ls, 2); lua_pop(ls, 1); return 0; }
        n++;
        lua_pop(ls, 1); /* pop value, keep key for next iteration */
    }
    lua_pop(ls, 1); /* pop the table copy */
    return 1;
}

static void json_encode_string(lua_State *ls, const char *s, size_t len, luaL_Buffer *b) {
    luaL_addchar(b, '"');
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char)s[i];
        switch (c) {
            case '"': luaL_addstring(b, "\\\""); break;
            case '\\': luaL_addstring(b, "\\\\"); break;
            case '\b': luaL_addstring(b, "\\b"); break;
            case '\f': luaL_addstring(b, "\\f"); break;
            case '\n': luaL_addstring(b, "\\n"); break;
            case '\r': luaL_addstring(b, "\\r"); break;
            case '\t': luaL_addstring(b, "\\t"); break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    snprintf(buf, sizeof(buf), "\\u%04x", c);
                    luaL_addstring(b, buf);
                } else {
                    luaL_addchar(b, c);
                }
                break;
        }
    }
    luaL_addchar(b, '"');
}

static void json_encode_value(lua_State *ls, int idx, luaL_Buffer *b, int depth) {
    if (depth > 32) { luaL_addstring(b, "null"); return; }
    int t = lua_type(ls, idx);
    switch (t) {
        case LUA_TNIL:
            luaL_addstring(b, "null");
            break;
        case LUA_TBOOLEAN:
            luaL_addstring(b, lua_toboolean(ls, idx) ? "true" : "false");
            break;
        case LUA_TNUMBER:
            if (lua_isinteger(ls, idx)) {
                char buf[32];
                snprintf(buf, sizeof(buf), "%lld", (long long)lua_tointeger(ls, idx));
                luaL_addstring(b, buf);
            } else {
                double d = lua_tonumber(ls, idx);
                if (!isfinite(d)) { luaL_addstring(b, "null"); break; }
                char buf[64];
                snprintf(buf, sizeof(buf), "%.17g", d);
                luaL_addstring(b, buf);
            }
            break;
        case LUA_TSTRING: {
            size_t len;
            const char *s = lua_tolstring(ls, idx, &len);
            json_encode_string(ls, s, len, b);
            break;
        }
        case LUA_TTABLE: {
            if (is_array_table(ls, idx)) {
                luaL_addchar(b, '[');
                lua_Integer len = luaL_len(ls, idx);
                for (lua_Integer i = 1; i <= len; i++) {
                    if (i > 1) luaL_addchar(b, ',');
                    lua_rawgeti(ls, idx, i);
                    json_encode_value(ls, lua_gettop(ls), b, depth + 1);
                    lua_pop(ls, 1);
                }
                luaL_addchar(b, ']');
            } else {
                luaL_addchar(b, '{');
                int first = 1;
                lua_pushvalue(ls, idx);
                lua_pushnil(ls);
                while (lua_next(ls, -2) != 0) {
                    if (!first) luaL_addchar(b, ',');
                    first = 0;
                    /* key at -2 */
                    if (lua_type(ls, -2) == LUA_TSTRING) {
                        size_t klen;
                        const char *k = lua_tolstring(ls, -2, &klen);
                        json_encode_string(ls, k, klen, b);
                    } else if (lua_type(ls, -2) == LUA_TNUMBER) {
                        char kbuf[64];
                        if (lua_isinteger(ls, -2))
                            snprintf(kbuf, sizeof(kbuf), "\"%lld\"", (long long)lua_tointeger(ls, -2));
                        else
                            snprintf(kbuf, sizeof(kbuf), "\"%.17g\"", lua_tonumber(ls, -2));
                        luaL_addstring(b, kbuf);
                    }
                    luaL_addchar(b, ':');
                    json_encode_value(ls, -1, b, depth + 1);
                    lua_pop(ls, 1);
                }
                lua_pop(ls, 1); /* pop table copy */
                luaL_addchar(b, '}');
            }
            break;
        }
        default:
            luaL_addstring(b, "null");
            break;
    }
}

/* json_encode(lua_value) -> json_string */
static int c_json_encode(lua_State *ls) {
    luaL_Buffer b;
    luaL_buffinit(ls, &b);
    json_encode_value(ls, 1, &b, 0);
    luaL_pushresult(&b);
    return 1;
}

/* ---- Pre-compiled wrapper protocol ---- */

/* Args slot: 4KB buffer for passing JSON args without code interpolation */
#define ARGS_BUF_SIZE 4096
static char args_buf[ARGS_BUF_SIZE];
static size_t args_len = 0;

/* get_tool_args() - reads JSON from args_buf, returns Lua table */
static int c_get_tool_args(lua_State *ls) {
    JsonParser p = { .s = args_buf, .pos = 0, .len = args_len };
    jp_parse_value(&p, ls);
    return 1;
}

/* set_tool_result(value) - JSON-encodes Lua value to output_buf */
static int c_set_tool_result(lua_State *ls) {
    output_reset();
    luaL_Buffer b;
    luaL_buffinit(ls, &b);
    json_encode_value(ls, 1, &b, 0);
    luaL_pushresult(&b);
    size_t len;
    const char *s = lua_tolstring(ls, -1, &len);
    output_append(s, len);
    lua_pop(ls, 1);
    return 0;
}

/* Registry key for pre-compiled tool wrappers */
#define TOOL_REGISTRY_KEY "__tool_wrappers"

/* lua_wasi_register_wrapper(name_ptr, name_len, src_ptr, src_len) -> int
 * Pre-compiles wrapper source and stores in Lua registry keyed by tool name.
 */
int lua_wasi_register_wrapper(const char *name_ptr, size_t name_len,
                               const char *src_ptr, size_t src_len) {
    if (L == NULL) return -1;

    /* Compile the wrapper */
    int status = luaL_loadbuffer(L, src_ptr, src_len, "tool_wrapper");
    if (status != LUA_OK) {
        const char *err = lua_tostring(L, -1);
        if (err) output_append(err, strlen(err));
        lua_pop(L, 1);
        return -1;
    }
    /* Compiled function is now on stack top */

    /* Get or create the tool wrappers table in registry */
    lua_getfield(L, LUA_REGISTRYINDEX, TOOL_REGISTRY_KEY);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        lua_newtable(L);
        lua_pushvalue(L, -1);
        lua_setfield(L, LUA_REGISTRYINDEX, TOOL_REGISTRY_KEY);
    }
    /* Stack: compiled_func, wrappers_table */

    /* Set wrappers_table[name] = compiled_func */
    lua_pushlstring(L, name_ptr, name_len);
    lua_pushvalue(L, -3);  /* copy compiled_func */
    lua_rawset(L, -3);     /* table[name] = func */

    lua_pop(L, 2);  /* pop table and compiled_func */
    return 0;
}

/* lua_wasi_call_tool(args_ptr, args_n, name_ptr, name_len) -> int
 * Single-call protocol: reads args from WASM memory, looks up pre-compiled
 * wrapper, executes it. Replaces the old two-call set_args + call_tool pattern.
 */
int lua_wasi_call_tool(const char *args_ptr, size_t args_n,
                       const char *name_ptr, size_t name_len) {
    if (L == NULL) return -1;

    /* Copy args directly from WASM memory into our parse buffer */
    if (args_n >= ARGS_BUF_SIZE) args_n = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, args_ptr, args_n);
    args_buf[args_n] = '\0';
    args_len = args_n;

    output_reset();

    /* Get the wrappers table */
    lua_getfield(L, LUA_REGISTRYINDEX, TOOL_REGISTRY_KEY);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        output_append("No tools registered", 17);
        return -1;
    }

    /* Look up the tool by name */
    lua_pushlstring(L, name_ptr, name_len);
    lua_rawget(L, -2);
    if (lua_isnil(L, -1)) {
        lua_pop(L, 2);
        output_append("Unknown tool", 12);
        return -1;
    }

    int status = lua_pcall(L, 0, 0, 0);
    if (status != LUA_OK) {
        const char *err = lua_tostring(L, -1);
        if (err) output_append(err, strlen(err));
        lua_pop(L, 2);
        return -1;
    }

    lua_settop(L, 0);
    return 0;
}

/* lua_wasi_set_args(ptr, len) - write args to args_buf slot */
int lua_wasi_set_args(const char *ptr, size_t len) {
    if (len >= ARGS_BUF_SIZE) len = ARGS_BUF_SIZE - 1;
    memcpy(args_buf, ptr, len);
    args_buf[len] = '\0';
    args_len = len;
    return 0;
}

/* ---- Stubs for excluded libraries ---- */

/* loadlib.c excluded (needs dlopen); stub package lib */
int luaopen_package(lua_State *ls) {
    (void)ls;
    return 0;
}

/* loslib.c excluded (needs tmpnam); stub os lib */
int luaopen_os(lua_State *ls) {
    (void)ls;
    return 0;
}

/* liolib.c excluded (needs tmpfile); stub io lib */
int luaopen_io(lua_State *ls) {
    (void)ls;
    return 0;
}

/* ---- Lifecycle ---- */

/* Initialize Lua runtime */
int lua_wasi_init(void) {
    if (L != NULL) {
        lua_close(L);
    }

    L = luaL_newstate();
    if (L == NULL) {
        return -1;
    }

    luaL_openlibs(L);

    /* Override print to capture output */
    lua_pushcfunction(L, lua_wasi_print);
    lua_setglobal(L, "print");

    /* Register JSON functions */
    lua_pushcfunction(L, c_json_decode);
    lua_setglobal(L, "json_decode");
    lua_pushcfunction(L, c_json_encode);
    lua_setglobal(L, "json_encode");

    /* Register tool protocol builtins */
    lua_pushcfunction(L, c_get_tool_args);
    lua_setglobal(L, "get_tool_args");
    lua_pushcfunction(L, c_set_tool_result);
    lua_setglobal(L, "set_tool_result");

    output_reset();
    return 0;
}

/* Execute Lua source code */
int lua_wasi_exec(const char *src, size_t len) {
    if (L == NULL) {
        return -1;
    }

    output_reset();

    /* Execute the code */
    int status = luaL_loadbuffer(L, src, len, "input");
    if (status != LUA_OK) {
        const char *err = lua_tostring(L, -1);
        if (err) {
            output_append(err, strlen(err));
            output_append("\n", 1);
        }
        lua_pop(L, 1);
        return -1;
    }

    status = lua_pcall(L, 0, LUA_MULTRET, 0);
    if (status != LUA_OK) {
        const char *err = lua_tostring(L, -1);
        if (err) {
            output_append(err, strlen(err));
            output_append("\n", 1);
        }
        lua_pop(L, 1);
        return -1;
    }

    lua_settop(L, 0);
    return 0;
}

/* Get pointer to output buffer */
const char *lua_wasi_get_output(void) {
    return output_buf;
}

/* Get output length */
size_t lua_wasi_get_output_len(void) {
    return output_len;
}

/* Get pointer to output_buf start (for diagnostics) */
const char *lua_wasi_get_output_buf_start(void) {
    return output_buf;
}

/* Destroy Lua runtime */
void lua_wasi_destroy(void) {
    if (L != NULL) {
        lua_close(L);
        L = NULL;
    }
}
