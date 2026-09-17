import struct

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
    f.read(8)  # skip header

    while True:
        section_id_byte = f.read(1)
        if not section_id_byte:
            break
        section_id = section_id_byte[0]
        size = read_leb128(f)
        pos = f.tell()

        if section_id == 0:
            name_len = read_leb128(f)
            name = f.read(name_len).decode('utf-8', errors='replace')

            if name == 'target_features':
                count = read_leb128(f)
                print(f'target_features: {count} features')
                for i in range(count):
                    feat_len = read_leb128(f)
                    feat_name = f.read(feat_len).decode('utf-8', errors='replace')
                    print(f'  +{feat_name}')

        f.seek(pos + size)
