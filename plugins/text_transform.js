// Custom Node.js tool: text transformation
// Called by VELOCITY-MCP plugin system with args passed as CLI arguments
// Usage: node text_transform.js --action upper --input "hello world"

const args = process.argv.slice(2);
const params = {};
for (let i = 0; i < args.length; i += 2) {
    const key = args[i].replace(/^--/, '');
    params[key] = args[i + 1] || '';
}

const action = params.action || 'upper';
const input = params.input || '';

let result;
switch (action) {
    case 'upper':
        result = input.toUpperCase();
        break;
    case 'lower':
        result = input.toLowerCase();
        break;
    case 'reverse':
        result = input.split('').reverse().join('');
        break;
    case 'slug':
        result = input.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
        break;
    default:
        console.error(`Unknown action: ${action}`);
        process.exit(1);
}

// Output as JSON — VELOCITY-MCP captures stdout
console.log(JSON.stringify({ result, action, input_length: input.length }));
