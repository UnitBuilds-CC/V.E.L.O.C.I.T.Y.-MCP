package velocity_mcp

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gorilla/websocket"
)

// The server answers a JSON-RPC notification with an empty 204 on HTTP
// (src/transport/http.rs) and with nothing at all on WebSocket, because only
// id-bearing requests get a reply. Both are successes: there is simply no
// envelope to hand back.

func TestHttpTransportNotificationHasNoEnvelope(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		if strings.Contains(string(body), `"id"`) {
			w.Header().Set("Content-Type", "application/json")
			fmt.Fprint(w, `{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05"}}`)
			return
		}
		w.WriteHeader(http.StatusNoContent)
	}))
	defer server.Close()

	transport := NewHttpTransport(server.URL, "", 0)
	defer transport.Close()

	response, err := transport.Send(context.Background(),
		JsonRpcRequest{Jsonrpc: "2.0", Method: "notifications/initialized"})
	if err != nil {
		t.Fatalf("notification: want success, got %v", err)
	}
	if response != nil {
		t.Errorf("notification: want no envelope, got %+v", response)
	}

	// The request arm still round-trips, so the 204 case did not swallow it.
	response, err = transport.Send(context.Background(),
		JsonRpcRequest{Jsonrpc: "2.0", Method: "initialize", ID: 1})
	if err != nil {
		t.Fatalf("request: want success, got %v", err)
	}
	if response == nil {
		t.Fatal("request: want an envelope, got nil")
	}
}

// A real HTTP failure must stay a failure even when the client sent a
// notification, or the empty-response path would hide a refused request.
func TestHttpTransportNotificationErrorStatusStillFails(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
		fmt.Fprint(w, "unauthorized")
	}))
	defer server.Close()

	transport := NewHttpTransport(server.URL, "", 0)
	defer transport.Close()

	_, err := transport.Send(context.Background(),
		JsonRpcRequest{Jsonrpc: "2.0", Method: "notifications/initialized"})
	if err == nil {
		t.Fatal("notification over 401: want a transport error, got nil")
	}
	if _, ok := err.(*TransportError); !ok {
		t.Errorf("notification over 401: want *TransportError, got %T (%v)", err, err)
	}
}

func TestWebSocketTransportNotificationDoesNotBlock(t *testing.T) {
	upgrader := websocket.Upgrader{}
	server := httptest.NewUnstartedServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := upgrader.Upgrade(w, r, nil)
		if err != nil {
			return
		}
		go func() {
			defer conn.Close()
			for {
				var request JsonRpcRequest
				if err := conn.ReadJSON(&request); err != nil {
					return
				}
				if request.ID == nil {
					continue
				}
				reply := JsonRpcResponse{Jsonrpc: "2.0", ID: request.ID,
					Result: map[string]interface{}{"result": "ok"}}
				if err := conn.WriteJSON(&reply); err != nil {
					return
				}
			}
		}()
	}))
	server.Start()
	// The notification leg leaves its connection open on purpose, so both calls are
	// forced shut before the listener closes instead of waiting on them.
	defer func() {
		server.CloseClientConnections()
		server.Close()
	}()

	url := "ws" + strings.TrimPrefix(server.URL, "http")
	transport := NewWebSocketTransport(url, "")

	done := make(chan error, 1)
	go func() {
		_, err := transport.Send(context.Background(),
			JsonRpcRequest{Jsonrpc: "2.0", Method: "notifications/initialized"})
		done <- err
	}()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("notification: want success, got %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("notification: Send is still waiting for a reply the server never sends")
	}

	// The request arm still round-trips on the same connection.
	reply := make(chan *JsonRpcResponse, 1)
	go func() {
		response, err := transport.Send(context.Background(),
			JsonRpcRequest{Jsonrpc: "2.0", Method: "ping", ID: 7})
		if err != nil {
			done <- err
			return
		}
		reply <- response
	}()
	select {
	case response := <-reply:
		if response == nil {
			t.Fatal("request: want an envelope, got nil")
		}
	case err := <-done:
		t.Fatalf("request: want success, got %v", err)
	case <-time.After(5 * time.Second):
		t.Fatal("request: Send did not return")
	}
}

// Initialize sends the initialized notification on every transport, so a
// notification the transport cannot handle makes the whole handshake fail.
func TestInitializeOverHttpCompletes(t *testing.T) {
	var requests int
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		requests++
		if strings.Contains(string(body), `"id"`) {
			w.Header().Set("Content-Type", "application/json")
			fmt.Fprint(w, `{"jsonrpc":"2.0","id":1,"result":`+
				`{"protocolVersion":"2024-11-05","serverInfo":{"name":"test","version":"3.2.0"}}}`)
			return
		}
		w.WriteHeader(http.StatusNoContent)
	}))
	defer server.Close()

	client := NewMcpClient(NewHttpTransport(server.URL, "", 0))
	defer client.Close()

	result, err := client.Initialize(context.Background())
	if err != nil {
		t.Fatalf("Initialize: want success, got %v", err)
	}
	if result.ProtocolVersion != "2024-11-05" {
		t.Errorf("Initialize: want protocolVersion 2024-11-05, got %q", result.ProtocolVersion)
	}
	if !client.IsInitialized() {
		t.Error("Initialize: want IsInitialized true")
	}
	if requests != 2 {
		t.Errorf("Initialize: want the initialize request plus the initialized notification, saw %d requests", requests)
	}
}

// Initialize must not report success when the server refuses the initialized
// notification. Discarding that error is what kept the two transport defects
// above invisible from the client's point of view.
func TestInitializeSurfacesNotificationFailure(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		if strings.Contains(string(body), `"id"`) {
			w.Header().Set("Content-Type", "application/json")
			fmt.Fprint(w, `{"jsonrpc":"2.0","id":1,"result":`+
				`{"protocolVersion":"2024-11-05","serverInfo":{"name":"test","version":"3.2.0"}}}`)
			return
		}
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer server.Close()

	client := NewMcpClient(NewHttpTransport(server.URL, "", 0))
	defer client.Close()

	if _, err := client.Initialize(context.Background()); err == nil {
		t.Fatal("Initialize over a refused notification: want an error, got nil")
	}
	if client.IsInitialized() {
		t.Error("Initialize over a refused notification: want IsInitialized false")
	}
}

// sendRequest always stamps an id, so a 204 to an id-bearing request is a server that
// answered a request like a notification. That is a failure to report, not a nil
// pointer to read.
func TestSendRequestWithoutEnvelopeReportsAnError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	}))
	defer server.Close()

	client := NewMcpClient(NewHttpTransport(server.URL, "", 0))
	defer client.Close()

	if _, err := client.ListTools(context.Background()); err == nil {
		t.Fatal("id-bearing request answered with 204: want an error, got nil")
	} else if _, ok := err.(*TransportError); !ok {
		t.Errorf("id-bearing request answered with 204: want *TransportError, got %T (%v)", err, err)
	}
}
