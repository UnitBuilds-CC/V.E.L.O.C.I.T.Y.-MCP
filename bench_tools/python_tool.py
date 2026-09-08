import sys, json

def text_analyze(args):
    text = args.get('text', '')
    words = text.split()
    return {
        'word_count': len(words),
        'char_count': len(text),
        'line_count': len(text.split('\n')) if text else 0
    }

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        request = json.loads(line)
        args = request.get('params', {}).get('arguments', {})
        result = text_analyze(args)
        response = json.dumps({'jsonrpc': '2.0', 'result': result, 'id': request.get('id')})
        sys.stdout.write(response + '\n')
        sys.stdout.flush()
    except Exception as e:
        err = json.dumps({'jsonrpc': '2.0', 'error': {'code': -32603, 'message': str(e)}, 'id': None})
        sys.stdout.write(err + '\n')
        sys.stdout.flush()
