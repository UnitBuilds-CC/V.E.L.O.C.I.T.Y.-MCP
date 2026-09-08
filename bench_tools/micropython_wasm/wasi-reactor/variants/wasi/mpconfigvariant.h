/*
 * MicroPython WASI variant — minimal config for embedded WASM execution.
 * No JS interop, no VFS, no async. Just eval Python source and get results.
 */

#define MICROPY_VARIANT_ENABLE_JS_HOOK (0)

#define MICROPY_VFS (0)
#define MICROPY_PY_JS (0)
#define MICROPY_PY_JSFFI (0)
#define MICROPY_PY_ASYNCIO (0)

#define MICROPY_GC_SPLIT_HEAP (1)
#define MICROPY_GC_SPLIT_HEAP_AUTO (1)
