#!/bin/bash
# Build mruby as a WASI reactor manually, bypassing the rake build system.
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MRUBY_SRC="$SCRIPT_DIR/mruby-src"
BUILD_DIR="$SCRIPT_DIR/build_manual"
WASI_SDK="/c/wasi-sdk"
CLANG="$WASI_SDK/bin/clang"
RUBY="/c/Ruby33-x64/bin/ruby"

CORE_INCLUDES="-I$SCRIPT_DIR/wasm_compat -I$MRUBY_SRC/include"
COMPILER_INCLUDES="-I$MRUBY_SRC/mrbgems/mruby-compiler/include -I$MRUBY_SRC/mrbgems/mruby-compiler/lib/prism/include"
COMPILER_DEFINES="-DMRC_TARGET_MRUBY -DPRISM_XALLOCATOR -DPRISM_DEPTH_MAXIMUM=256 -DPRISM_BUILD_MINIMAL"
CFLAGS="-O2 -DWASM --target=wasm32-wasip1 $CORE_INCLUDES"

echo "=== Step 1: Presym scanning (preprocess all C files) ==="
rm -rf "$BUILD_DIR/scan" "$BUILD_DIR/obj"
mkdir -p "$BUILD_DIR/scan"

scan_file() {
    local src="$1" rel="$2" extra_includes="$3"
    local pi="$BUILD_DIR/scan/${rel}.pi"
    mkdir -p "$(dirname "$pi")"
    "$CLANG" -E -P $CFLAGS $extra_includes -DMRB_PRESYM_SCANNING -o "$pi" "$src" 2>/dev/null
    echo "$pi"
}

ALL_PI=""
# Core sources
for f in "$MRUBY_SRC"/src/*.c; do
    ALL_PI="$ALL_PI $(scan_file "$f" "core/$(basename "$f" .c)" "")"
done
# Gem sources
GEMS="mruby-compiler mruby-eval mruby-binding mruby-sprintf mruby-string-ext mruby-array-ext mruby-hash-ext mruby-enum-ext mruby-kernel-ext"
for gem in $GEMS; do
    gdir="$MRUBY_SRC/mrbgems/$gem/src"
    [ -d "$gdir" ] || continue
    for f in "$gdir"/*.c; do
        ALL_PI="$ALL_PI $(scan_file "$f" "$gem/$(basename "$f" .c)" "$COMPILER_INCLUDES")"
    done
done
echo "  Preprocessed $(echo "$ALL_PI" | wc -w) files"

echo "=== Step 2: Generate presym headers ==="
mkdir -p "$BUILD_DIR/include/mruby/presym"

"$RUBY" - "$BUILD_DIR" <<'RUBY_SCRIPT'
class String
  def relative_path; self; end
end
def _pp(*args); puts "  #{args.join(' ')}"; end

require_relative "mruby-src/lib/mruby/presym"

build_dir = ARGV[0]
build = Struct.new(:build_dir).new(build_dir)
presym = MRuby::Presym.new(build)

pi_files = Dir.glob("#{build_dir}/scan/**/*.pi")
presyms = presym.scan(pi_files)
puts "  Found #{presyms.size} presyms"
presym.write_list(presyms)
presym.write_id_header(presyms)
presym.write_table_header(presyms)
RUBY_SCRIPT

echo "=== Step 3: Compile all C files ==="
PRESYM_INCLUDES="-I$BUILD_DIR/include"
OBJ_FILES=""

compile_file() {
    local src="$1" obj="$2" extra_includes="$3"
    mkdir -p "$(dirname "$obj")"
    echo "  CC $(basename "$src")"
    "$CLANG" -c -O2 -DWASM --target=wasm32-wasip1 \
        $CORE_INCLUDES $PRESYM_INCLUDES $extra_includes -o "$obj" "$src"
}

# Core
for f in "$MRUBY_SRC"/src/*.c; do
    base=$(basename "$f" .c)
    obj="$BUILD_DIR/obj/core/${base}.o"
    compile_file "$f" "$obj" ""
    OBJ_FILES="$OBJ_FILES $obj"
done

# Gems (compiler gem needs extra defines)
for gem in $GEMS; do
    gdir="$MRUBY_SRC/mrbgems/$gem/src"
    [ -d "$gdir" ] || continue
    if [ "$gem" = "mruby-compiler" ]; then
        EXTRA="$COMPILER_INCLUDES $COMPILER_DEFINES"
    else
        EXTRA="$COMPILER_INCLUDES"
    fi
    for f in "$gdir"/*.c; do
        base=$(basename "$f" .c)
        obj="$BUILD_DIR/obj/gems/$gem/${base}.o"
        compile_file "$f" "$obj" "$EXTRA"
        OBJ_FILES="$OBJ_FILES $obj"
    done
done

# Prism sources (parser library used by mruby-compiler)
PRISM_DIR="$MRUBY_SRC/mrbgems/mruby-compiler/lib/prism"
PRISM_INCLUDES="-I$PRISM_DIR/include -I$MRUBY_SRC/mrbgems/mruby-compiler/include"
for f in "$PRISM_DIR"/src/*.c; do
    base=$(basename "$f" .c)
    obj="$BUILD_DIR/obj/prism/${base}.o"
    compile_file "$f" "$obj" "$PRISM_INCLUDES"
    OBJ_FILES="$OBJ_FILES $obj"
done
# Prism util sources
for f in "$PRISM_DIR"/src/util/*.c; do
    base=$(basename "$f" .c)
    obj="$BUILD_DIR/obj/prism/util/${base}.o"
    compile_file "$f" "$obj" "$PRISM_INCLUDES"
    OBJ_FILES="$OBJ_FILES $obj"
done

# Wrapper
echo "  CC mruby_wasi.c"
mkdir -p "$BUILD_DIR/obj"
"$CLANG" -c -O2 -DWASM --target=wasm32-wasip1 \
    $CORE_INCLUDES $PRESYM_INCLUDES -o "$BUILD_DIR/obj/mruby_wasi.o" "$SCRIPT_DIR/mruby_wasi.c"
OBJ_FILES="$OBJ_FILES $BUILD_DIR/obj/mruby_wasi.o"

# gem_init
echo "  CC gem_init.c"
"$CLANG" -c -O2 -DWASM --target=wasm32-wasip1 \
    $CORE_INCLUDES $PRESYM_INCLUDES -o "$BUILD_DIR/obj/gem_init.o" "$BUILD_DIR/gem_init.c"
OBJ_FILES="$OBJ_FILES $BUILD_DIR/obj/gem_init.o"

# Custom setjmp/longjmp (no WASM EH)
echo "  CC setjmp.c (wasm_compat)"
"$CLANG" -c -O2 -DWASM --target=wasm32-wasip1 \
    -I"$SCRIPT_DIR/wasm_compat" -o "$BUILD_DIR/obj/setjmp.o" "$SCRIPT_DIR/wasm_compat/setjmp.c"
OBJ_FILES="$OBJ_FILES $BUILD_DIR/obj/setjmp.o"

echo "=== Step 4: Link WASI reactor ==="
echo "  LD ruby.wasm ($(echo $OBJ_FILES | wc -w) object files)"
# shellcheck disable=SC2086
"$CLANG" --target=wasm32-wasip1 \
    -Wl,--no-entry -Wl,--export-all \
    -o "$SCRIPT_DIR/ruby.wasm" $OBJ_FILES -lm

echo "=== Done ==="
ls -lh "$SCRIPT_DIR/ruby.wasm"
