/*
 * Shared Interpreter Core - Header
 * Tree-walk interpreter with dialect-driven language frontends.
 * Supports PHP, C#, Java, R, Julia, Perl syntax variants.
 */

#ifndef INTERP_H
#define INTERP_H

#include <stddef.h>

/* ============================================================================
 * Configuration Constants
 * ============================================================================ */

#define INTERP_MAX_STRING_LEN   (64 * 1024)
#define INTERP_MAX_ARRAY_SIZE   1024
#define INTERP_MAX_NEST_DEPTH   64
#define INTERP_ARENA_SIZE       (64 * 1024)
#define INTERP_SCOPE_BUCKETS    16
#define INTERP_ERROR_BUF_SIZE   512

/* ============================================================================
 * Forward Declarations
 * ============================================================================ */

typedef struct Value Value;
typedef struct ArrayVal ArrayVal;
typedef struct MapVal MapVal;
typedef struct MapEntry MapEntry;
typedef struct FuncDef FuncDef;
typedef struct AstNode AstNode;
typedef struct Scope Scope;
typedef struct ScopeEntry ScopeEntry;
typedef struct InterpDialect InterpDialect;

/* ============================================================================
 * Value Types
 * ============================================================================ */

typedef enum {
    VAL_NULL,
    VAL_BOOL,
    VAL_NUMBER,
    VAL_STRING,
    VAL_ARRAY,
    VAL_MAP,
    VAL_FUNC
} ValType;

struct ArrayVal {
    Value **items;
    int len;
    int cap;
};

struct MapEntry {
    char *key;
    Value *value;
    MapEntry *next;
};

struct MapVal {
    MapEntry **buckets;
    int num_buckets;
    int size;
};

typedef struct {
    char *name;
} Param;

struct FuncDef {
    char *name;
    Param *params;
    int num_params;
    AstNode *body;
    Scope *closure;
};

struct Value {
    ValType type;
    int refcount;
    union {
        int boolean;
        double number;
        struct {
            char *data;
            size_t len;
        } str;
        ArrayVal *arr;
        MapVal *map;
        FuncDef *func;
    } as;
};

/* ============================================================================
 * AST Node Types
 * ============================================================================ */

typedef enum {
    AST_NUMBER_LIT,
    AST_STRING_LIT,
    AST_BOOL_LIT,
    AST_NULL_LIT,
    AST_ARRAY_LIT,
    AST_MAP_LIT,
    AST_INTERPOLATED_STR,
    AST_IDENT,
    AST_BINARY,
    AST_UNARY,
    AST_CALL,
    AST_INDEX,
    AST_MEMBER,
    AST_BLOCK,
    AST_ASSIGN,
    AST_IF,
    AST_WHILE,
    AST_FOR,
    AST_RETURN,
    AST_EXPR_STMT,
    AST_FUNC_DEF,
    AST_PRINT
} AstNodeType;

typedef struct {
    AstNode **elements;
    int count;
} ArrayLit;

typedef struct {
    AstNode **keys;
    AstNode **values;
    int count;
} MapLit;

typedef struct {
    AstNode **parts;
    int count;
} InterpStr;

typedef struct {
    int op;
    AstNode *left;
    AstNode *right;
} Binary;

typedef struct {
    int op;
    AstNode *operand;
} Unary;

typedef struct {
    AstNode *func;
    AstNode **args;
    int num_args;
} Call;

typedef struct {
    AstNode *object;
    AstNode *index;
} Index;

typedef struct {
    AstNode *object;
    char *member;
} Member;

typedef struct {
    AstNode **stmts;
    int count;
} Block;

typedef struct {
    AstNode *target;
    AstNode *value;
} Assign;

typedef struct {
    AstNode *cond;
    AstNode *then_body;
    AstNode *else_body;
} IfStmt;

typedef struct {
    AstNode *cond;
    AstNode *body;
} WhileStmt;

typedef struct {
    AstNode *init;
    AstNode *cond;
    AstNode *step;
    AstNode *body;
} ForStmt;

typedef struct {
    AstNode *value;
} ReturnStmt;

typedef struct {
    AstNode *expr;
} ExprStmt;

typedef struct {
    char *name;
    char **params;
    int num_params;
    AstNode *body;
} FuncDefNode;

typedef struct {
    AstNode *value;
} PrintStmt;

struct AstNode {
    AstNodeType type;
    int line;
    int col;
    union {
        double number;
        char *str;
        int boolean;
        ArrayLit array_lit;
        MapLit map_lit;
        InterpStr interp_str;
        char *ident;
        Binary binary;
        Unary unary;
        Call call;
        Index index;
        Member member;
        Block block;
        Assign assign;
        IfStmt if_stmt;
        WhileStmt while_stmt;
        ForStmt for_stmt;
        ReturnStmt return_stmt;
        ExprStmt expr_stmt;
        FuncDefNode func_def;
        PrintStmt print_stmt;
    } as;
};

/* ============================================================================
 * Token Types
 * ============================================================================ */

typedef enum {
    TOK_NUMBER,
    TOK_STRING,
    TOK_IDENT,
    TOK_TRUE,
    TOK_FALSE,
    TOK_NULL,
    TOK_IF,
    TOK_ELSE,
    TOK_WHILE,
    TOK_FOR,
    TOK_FUNCTION,
    TOK_RETURN,
    TOK_PRINT,
    TOK_IN,
    TOK_PLUS,
    TOK_MINUS,
    TOK_STAR,
    TOK_SLASH,
    TOK_PERCENT,
    TOK_DOT,
    TOK_EQ,
    TOK_NEQ,
    TOK_LT,
    TOK_GT,
    TOK_LTE,
    TOK_GTE,
    TOK_AND,
    TOK_OR,
    TOK_NOT,
    TOK_ASSIGN,
    TOK_ARROW,
    TOK_LPAREN,
    TOK_RPAREN,
    TOK_LBRACKET,
    TOK_RBRACKET,
    TOK_LBRACE,
    TOK_RBRACE,
    TOK_COMMA,
    TOK_SEMICOLON,
    TOK_COLON,
    TOK_NEWLINE,
    TOK_EOF
} TokenType;

typedef struct {
    TokenType type;
    const char *start;
    size_t len;
    int line;
    int col;
    double num_val;
} Token;

/* ============================================================================
 * Dialect Configuration
 * ============================================================================ */

struct InterpDialect {
    const char *prefix;
    int line_comment_hash;
    int line_comment_slash;
    int block_comment;
    int var_prefix;
    int assign_arrow;
    int semicolon_required;
    int interp_style;
    int concat_dot;
    int brace_blocks;
    const char *skip_prefix;
    const char *func_keyword;
    int return_parens;
    const char *type_keywords[8];
    int type_annotations;
    const char *print_keywords[8];
};

/* ============================================================================
 * Scope (Variable Environment)
 * ============================================================================ */

struct ScopeEntry {
    char *name;
    Value *value;
    ScopeEntry *next;
};

struct Scope {
    ScopeEntry **buckets;
    int num_buckets;
    int size;
    Scope *parent;
};

/* ============================================================================
 * Core API
 * ============================================================================ */

void interp_init(InterpDialect *dialect);
void interp_reset(void);
void interp_destroy(void);

int interp_exec(const char *source);

void interp_register_tool(const char *name_ptr, size_t name_len,
                          const char *src_ptr, size_t src_len);
int interp_call_tool(const char *args_ptr, size_t args_len,
                     const char *name_ptr, size_t name_len);

const char *interp_get_output(void);
size_t interp_get_output_len(void);

void interp_set_result(const char *value);
const char *interp_get_result(void);

#endif /* INTERP_H */
