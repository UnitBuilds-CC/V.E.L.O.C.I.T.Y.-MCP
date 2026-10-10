package velocity_mcp

import (
	"context"
	"errors"
	"testing"
)

// A transport that replays one canned response, so the classification logic in
// ReadResource and GetPrompt can be exercised without a server.
type cannedTransport struct {
	response *JsonRpcResponse
}

func (t *cannedTransport) Send(ctx context.Context, request JsonRpcRequest) (*JsonRpcResponse, error) {
	return t.response, nil
}

func (t *cannedTransport) Close() error { return nil }

func inBandResult(text string) *JsonRpcResponse {
	return &JsonRpcResponse{
		Jsonrpc: "2.0",
		ID:      1,
		Result: map[string]interface{}{
			"content": []interface{}{map[string]interface{}{
				"type": "text",
				"text": text,
			}},
			"isError": true,
		},
	}
}

func errorResult(code int, message string) *JsonRpcResponse {
	return &JsonRpcResponse{
		Jsonrpc: "2.0",
		ID:      1,
		Error:   &JsonRpcError{Code: code, Message: message},
	}
}

// The native server (src/protocol/json_rpc.rs) reports a failed resource or prompt
// read in band; velocity-mcp-core (crates/velocity-mcp-core/src/lib.rs) reports the
// not-found case as a JSON-RPC -32001 error object. Both must reach the typed error
// the method promises, and a different in-band cause must not be relabelled.
func TestReadResourceClassification(t *testing.T) {
	ok := &JsonRpcResponse{Jsonrpc: "2.0", ID: 1, Result: map[string]interface{}{
		"contents": []interface{}{map[string]interface{}{
			"uri": "file:///a.txt", "text": "hi"}},
	}}

	cases := []struct {
		name     string
		response *JsonRpcResponse
		wantErr  error
		wantOK   bool
	}{
		{"native not found", inBandResult("Resource not found: invalid://nope"),
			&ResourceNotFoundError{}, false},
		{"native io failure", inBandResult("Failed to open file: os error 3"),
			&ServerError{}, false},
		{"core not found", errorResult(-32001, "Resource not found: nope"),
			&ResourceNotFoundError{}, false},
		{"other protocol error", errorResult(-32601, "Method not found"),
			&ProtocolError{}, false},
		{"success", ok, nil, true},
	}

	for _, c := range cases {
		client := NewMcpClient(&cannedTransport{response: c.response})
		result, err := client.ReadResource(context.Background(), "invalid://nope")
		if c.wantOK {
			if err != nil {
				t.Errorf("%s: want success, got %v", c.name, err)
			}
			if len(result.Contents) != 1 {
				t.Errorf("%s: want 1 content, got %d", c.name, len(result.Contents))
			}
			continue
		}
		var notFound *ResourceNotFoundError
		var server *ServerError
		var proto *ProtocolError
		switch c.wantErr.(type) {
		case *ResourceNotFoundError:
			if !errors.As(err, &notFound) {
				t.Errorf("%s: want *ResourceNotFoundError, got %T (%v)", c.name, err, err)
			} else if notFound.URI != "invalid://nope" {
				t.Errorf("%s: want URI carried, got %q", c.name, notFound.URI)
			}
		case *ServerError:
			if !errors.As(err, &server) {
				t.Errorf("%s: want *ServerError, got %T (%v)", c.name, err, err)
			} else if errors.As(err, &notFound) {
				t.Errorf("%s: a read failure was relabelled as not found", c.name)
			}
		case *ProtocolError:
			if !errors.As(err, &proto) {
				t.Errorf("%s: want *ProtocolError, got %T (%v)", c.name, err, err)
			}
		}
	}
}

func TestGetPromptClassification(t *testing.T) {
	ok := &JsonRpcResponse{Jsonrpc: "2.0", ID: 1, Result: map[string]interface{}{
		"description": "a prompt",
		"messages":    []interface{}{},
	}}

	cases := []struct {
		name     string
		response *JsonRpcResponse
		want     string
	}{
		{"native not found", inBandResult("Prompt not found: missing_prompt"), "notfound"},
		{"native bad arguments", inBandResult("Missing required argument: topic"), "server"},
		{"core not found", errorResult(-32001, "Prompt not found: missing_prompt"), "notfound"},
		{"other protocol error", errorResult(-32601, "Method not found"), "protocol"},
		{"success", ok, "ok"},
	}

	for _, c := range cases {
		client := NewMcpClient(&cannedTransport{response: c.response})
		result, err := client.GetPrompt(context.Background(), "missing_prompt", map[string]interface{}{})
		var notFound *PromptNotFoundError
		var server *ServerError
		var proto *ProtocolError
		switch c.want {
		case "ok":
			if err != nil {
				t.Errorf("%s: want success, got %v", c.name, err)
			}
			if result.Description != "a prompt" {
				t.Errorf("%s: want description carried, got %q", c.name, result.Description)
			}
		case "notfound":
			if !errors.As(err, &notFound) {
				t.Errorf("%s: want *PromptNotFoundError, got %T (%v)", c.name, err, err)
			} else if notFound.Name != "missing_prompt" {
				t.Errorf("%s: want Name carried, got %q", c.name, notFound.Name)
			}
		case "server":
			if !errors.As(err, &server) {
				t.Errorf("%s: want *ServerError, got %T (%v)", c.name, err, err)
			} else if errors.As(err, &notFound) {
				t.Errorf("%s: an argument failure was relabelled as not found", c.name)
			}
		case "protocol":
			if !errors.As(err, &proto) {
				t.Errorf("%s: want *ProtocolError, got %T (%v)", c.name, err, err)
			}
		}
	}
}
