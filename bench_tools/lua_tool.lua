-- Native Lua 5.4 stdio JSON-RPC tool server for benchmark comparison.
-- Minimal JSON parser for the known input schema: {"params":{"arguments":{"text":"..."}}}

local function extract_text(json_str)
    local text = json_str:match('"text"%s*:%s*"(.*)"')
    if text then
        text = text:gsub('\\"', '"'):gsub('\\\\', '\\'):gsub('\\n', '\n'):gsub('\\t', '\t')
    end
    return text or ''
end

local function text_analyze(text)
    local word_count = 0
    for _ in text:gmatch('%S+') do word_count = word_count + 1 end
    local char_count = #text
    local line_count = 0
    if #text > 0 then
        for _ in text:gmatch('\n') do line_count = line_count + 1 end
        line_count = line_count + 1
    end
    return word_count, char_count, line_count
end

local function extract_id(json_str)
    local id = json_str:match('"id"%s*:%s*(%d+)')
    return id or 'null'
end

for line in io.lines() do
    if line == '' then goto continue end
    local text = extract_text(line)
    local wc, cc, lc = text_analyze(text)
    local id = extract_id(line)
    local response = string.format(
        '{"jsonrpc":"2.0","result":{"word_count":%d,"char_count":%d,"line_count":%d},"id":%s}',
        wc, cc, lc, id
    )
    io.write(response .. '\n')
    io.flush()
    ::continue::
end
