import struct, sys

def read_leb128(f):
    size = 0
    shift = 0
    while True:
        b = f.read(1)[0]
        size |= (b & 0x7f) << shift
        if not (b & 0x80):
            break
        shift += 7
    return size

with open('crates/velocity-mcp-edge/velocity-edge-wasix.wasm', 'rb') as f:
    magic = f.read(4)
    version = struct.unpack('<I', f.read(4))[0]
    print(f'Magic: {magic.hex()} ({magic})')
    print(f'Version: {version}')

    while True:
        section_id_byte = f.read(1)
        if not section_id_byte:
            break
        section_id = section_id_byte[0]
        size = read_leb128(f)
        pos = f.tell()

        if section_id == 5:  # Memory section
            count = read_leb128(f)
            print(f'\nMemory section: {count} memories')
            for i in range(count):
                flags = f.read(1)[0]
                has_max = flags & 1
                is_shared = flags & 2
                initial = read_leb128(f)
                maximum = read_leb128(f) if has_max else 'none'
                print(f'  Memory {i}: flags={flags:#x} (shared={bool(is_shared)}), initial={initial} pages ({initial*64}KB), max={maximum}')

        elif section_id == 2:  # Import section
            count = read_leb128(f)
            imports = []
            for i in range(count):
                mod_len = read_leb128(f)
                mod_name = f.read(mod_len).decode('utf-8', errors='replace')
                name_len = read_leb128(f)
                name = f.read(name_len).decode('utf-8', errors='replace')
                kind = f.read(1)[0]
                kind_name = {0: 'func', 1: 'table', 2: 'memory', 3: 'global'}.get(kind, f'?({kind})')
                if kind == 0:
                    idx = read_leb128(f)
                elif kind == 2:
                    flags = f.read(1)[0]
                    initial = read_leb128(f)
                    if flags & 1:
                        maximum = read_leb128(f)
                elif kind == 3:
                    f.read(2)
                elif kind == 1:
                    f.read(1)
                    flags = f.read(1)[0]
                    read_leb128(f)
                    if flags & 1:
                        read_leb128(f)
                imports.append((mod_name, name, kind_name))

            thread_imports = [(m,n,k) for m,n,k in imports if 'thread' in n.lower()]
            atomic_imports = [(m,n,k) for m,n,k in imports if 'atomic' in n.lower()]
            memory_imports = [(m,n,k) for m,n,k in imports if 'memory' in n.lower()]

            print(f'\nImport section: {count} imports')
            if thread_imports:
                print(f'  Thread imports ({len(thread_imports)}):')
                for m,n,k in thread_imports:
                    print(f'    {m}.{n} ({k})')
            if atomic_imports:
                print(f'  Atomic imports ({len(atomic_imports)}):')
                for m,n,k in atomic_imports:
                    print(f'    {m}.{n} ({k})')
            if memory_imports:
                print(f'  Memory-related imports ({len(memory_imports)}):')
                for m,n,k in memory_imports:
                    print(f'    {m}.{n} ({k})')

            wasi_imports = [(m,n,k) for m,n,k in imports if 'wasi' in m.lower()]
            print(f'  WASI imports: {len(wasi_imports)}')
            for m,n,k in wasi_imports[:30]:
                print(f'    {m}.{n} ({k})')
            if len(wasi_imports) > 30:
                print(f'    ... and {len(wasi_imports)-30} more')

        elif section_id == 7:  # Export section
            count = read_leb128(f)
            exports = []
            for i in range(count):
                name_len = read_leb128(f)
                name = f.read(name_len).decode('utf-8', errors='replace')
                kind = f.read(1)[0]
                idx = read_leb128(f)
                exports.append(name)

            key_exports = [e for e in exports if e in ('_start', 'main', 'handle_http_request', 'wasmer_free', '__wasm_call_ctors', '_initialize')]
            print(f'\nExport section: {count} exports')
            print(f'  Key exports found: {key_exports}')
            # Show all non-function exports that might be interesting
            interesting = [e for e in exports if not e.startswith('__') and ('thread' in e.lower() or 'memory' in e.lower() or 'start' in e.lower() or 'handle' in e.lower())]
            if interesting:
                print(f'  Interesting exports: {interesting}')

        elif section_id == 0:  # Custom section
            name_len = read_leb128(f)
            name = f.read(name_len).decode('utf-8', errors='replace')
            if any(kw in name.lower() for kw in ['target', 'wasi', 'wasix', 'thread', 'producers', 'name']):
                print(f'\nCustom section: "{name}" (size={size})')

        f.seek(pos + size)
