const readline = require('readline');

const rl = readline.createInterface({ input: process.stdin, output: process.stdout });

function textAnalyze(input) {
    const text = input.text || '';
    return {
        word_count: text.split(/\s+/).filter(w => w.length > 0).length,
        char_count: text.length,
        line_count: text.length === 0 ? 0 : text.split('\n').length,
    };
}

function benchEcho(input) {
    const size = input.size || 64;
    const payload = 'x'.repeat(Math.max(0, size));
    return { payload, size: payload.length };
}

function handleLine(line) {
    try {
        const request = JSON.parse(line);
        const toolName = request.params && request.params.name;
        const args = request.params && request.params.arguments || {};
        let result;
        if (toolName === 'bench_echo') {
            result = benchEcho(args);
        } else {
            result = textAnalyze(args);
        }
        const response = JSON.stringify({ jsonrpc: '2.0', result, id: request.id });
        process.stdout.write(response + '\n');
    } catch (e) {
        const err = JSON.stringify({ jsonrpc: '2.0', error: { code: -32603, message: e.message }, id: null });
        process.stdout.write(err + '\n');
    }
}

rl.on('line', handleLine);
