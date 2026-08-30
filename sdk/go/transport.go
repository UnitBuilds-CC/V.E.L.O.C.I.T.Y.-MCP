package velocity_mcp

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

// Transport is the interface for MCP transports
type Transport interface {
	Send(ctx context.Context, request JsonRpcRequest) (*JsonRpcResponse, error)
	Close() error
}

// HttpTransport implements HTTP transport
type HttpTransport struct {
	url     string
	apiKey  string
	client  *http.Client
	timeout time.Duration
}

// NewHttpTransport creates a new HTTP transport
func NewHttpTransport(url string, apiKey string, timeout time.Duration) *HttpTransport {
	if timeout == 0 {
		timeout = 30 * time.Second
	}
	return &HttpTransport{
		url:     url,
		apiKey:  apiKey,
		client:  &http.Client{Timeout: timeout},
		timeout: timeout,
	}
}

// Send sends a JSON-RPC request over HTTP
func (t *HttpTransport) Send(ctx context.Context, request JsonRpcRequest) (*JsonRpcResponse, error) {
	body, err := json.Marshal(request)
	if err != nil {
		return nil, &TransportError{McpError{Message: fmt.Sprintf("failed to marshal request: %v", err)}}
	}

	req, err := http.NewRequestWithContext(ctx, "POST", t.url, bytes.NewReader(body))
	if err != nil {
		return nil, &TransportError{McpError{Message: fmt.Sprintf("failed to create request: %v", err)}}
	}

	req.Header.Set("Content-Type", "application/json")
	if t.apiKey != "" {
		req.Header.Set("Authorization", "Bearer "+t.apiKey)
	}

	resp, err := t.client.Do(req)
	if err != nil {
		return nil, &TransportError{McpError{Message: fmt.Sprintf("failed to send request: %v", err)}}
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, &TransportError{McpError{Message: fmt.Sprintf("HTTP error %d: %s", resp.StatusCode, string(body))}}
	}

	var rpcResp JsonRpcResponse
	if err := json.NewDecoder(resp.Body).Decode(&rpcResp); err != nil {
		return nil, &TransportError{McpError{Message: fmt.Sprintf("failed to decode response: %v", err)}}
	}

	return &rpcResp, nil
}

// Close closes the HTTP transport
func (t *HttpTransport) Close() error {
	return nil
}

// WebSocketTransport implements WebSocket transport
type WebSocketTransport struct {
	url    string
	apiKey string
	conn   *websocket.Conn
	mu     sync.Mutex
}

// NewWebSocketTransport creates a new WebSocket transport
func NewWebSocketTransport(url string, apiKey string) *WebSocketTransport {
	return &WebSocketTransport{
		url:    url,
		apiKey: apiKey,
	}
}

// Send sends a JSON-RPC request over WebSocket
func (t *WebSocketTransport) Send(ctx context.Context, request JsonRpcRequest) (*JsonRpcResponse, error) {
	t.mu.Lock()
	defer t.mu.Unlock()

	// Connect if not connected
	if t.conn == nil {
		header := http.Header{}
		if t.apiKey != "" {
			header.Set("Authorization", "Bearer "+t.apiKey)
		}
		
		conn, _, err := websocket.DefaultDialer.DialContext(ctx, t.url, header)
		if err != nil {
			return nil, &TransportError{McpError{Message: fmt.Sprintf("failed to connect: %v", err)}}
		}
		t.conn = conn
	}

	// Send request
	body, err := json.Marshal(request)
	if err != nil {
		return nil, &TransportError{McpError{Message: fmt.Sprintf("failed to marshal request: %v", err)}}
	}

	if err := t.conn.WriteMessage(websocket.TextMessage, body); err != nil {
		return nil, &TransportError{McpError{Message: fmt.Sprintf("failed to send request: %v", err)}}
	}

	// Receive response
	_, message, err := t.conn.ReadMessage()
	if err != nil {
		return nil, &TransportError{McpError{Message: fmt.Sprintf("failed to receive response: %v", err)}}
	}

	var rpcResp JsonRpcResponse
	if err := json.Unmarshal(message, &rpcResp); err != nil {
		return nil, &TransportError{McpError{Message: fmt.Sprintf("failed to decode response: %v", err)}}
	}

	return &rpcResp, nil
}

// Close closes the WebSocket transport
func (t *WebSocketTransport) Close() error {
	t.mu.Lock()
	defer t.mu.Unlock()
	
	if t.conn != nil {
		return t.conn.Close()
	}
	return nil
}
