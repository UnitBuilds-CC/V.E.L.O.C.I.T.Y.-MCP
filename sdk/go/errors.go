package velocity_mcp

import "fmt"

// McpError is the base error type for MCP client errors
type McpError struct {
	Message string
}

func (e *McpError) Error() string {
	return e.Message
}

// TransportError represents transport-level errors
type TransportError struct {
	McpError
}

// ProtocolError represents JSON-RPC protocol errors
type ProtocolError struct {
	McpError
	Code int
	Data interface{}
}

// ServerError represents server errors
type ServerError struct {
	McpError
}

// InvalidParameterError represents invalid parameter errors
type InvalidParameterError struct {
	McpError
}

// ConnectionClosedError represents connection closed errors
type ConnectionClosedError struct {
	McpError
}

// TimeoutError represents timeout errors
type TimeoutError struct {
	McpError
}

// ToolExecutionError represents tool execution errors
type ToolExecutionError struct {
	McpError
}

// ResourceNotFoundError represents resource not found errors
type ResourceNotFoundError struct {
	McpError
	URI string
}

func (e *ResourceNotFoundError) Error() string {
	return fmt.Sprintf("Resource not found: %s", e.URI)
}

// PromptNotFoundError represents prompt not found errors
type PromptNotFoundError struct {
	McpError
	Name string
}

func (e *PromptNotFoundError) Error() string {
	return fmt.Sprintf("Prompt not found: %s", e.Name)
}
