package velocity_mcp

import (
	"context"
	"fmt"
	"sync/atomic"
)

const (
	ProtocolVersion = "2024-11-05"
	ClientName      = "velocity-mcp-go"
	ClientVersion   = "3.0.0"
)

// McpClient is the main client for interacting with MCP servers
type McpClient struct {
	transport   Transport
	requestID   atomic.Int64
	initialized bool
}

// NewMcpClient creates a new MCP client
func NewMcpClient(transport Transport) *McpClient {
	return &McpClient{
		transport: transport,
	}
}

// nextID returns the next request ID
func (c *McpClient) nextID() int64 {
	return c.requestID.Add(1)
}

// sendRequest sends a JSON-RPC request and returns the result
func (c *McpClient) sendRequest(ctx context.Context, method string, params interface{}) (interface{}, error) {
	request := JsonRpcRequest{
		Jsonrpc: "2.0",
		Method:  method,
		Params:  params,
		ID:      c.nextID(),
	}

	response, err := c.transport.Send(ctx, request)
	if err != nil {
		return nil, err
	}

	if response.Error != nil {
		return nil, &ProtocolError{
			McpError: McpError{Message: fmt.Sprintf("Protocol error %d: %s", response.Error.Code, response.Error.Message)},
			Code:     response.Error.Code,
			Data:     response.Error.Data,
		}
	}

	return response.Result, nil
}

// Initialize initializes the MCP connection
func (c *McpClient) Initialize(ctx context.Context) (*InitializeResult, error) {
	params := InitializeParams{
		ProtocolVersion: ProtocolVersion,
		Capabilities:    ClientCapabilities{},
		ClientInfo: ClientInfo{
			Name:    ClientName,
			Version: ClientVersion,
		},
	}

	result, err := c.sendRequest(ctx, "initialize", params)
	if err != nil {
		return nil, err
	}

	// Send initialized notification
	notifRequest := JsonRpcRequest{
		Jsonrpc: "2.0",
		Method:  "notifications/initialized",
	}
	c.transport.Send(ctx, notifRequest)

	c.initialized = true

	// Convert result to InitializeResult
	resultMap, ok := result.(map[string]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid initialize result"}}
	}

	initResult := &InitializeResult{}
	if protocolVersion, ok := resultMap["protocolVersion"].(string); ok {
		initResult.ProtocolVersion = protocolVersion
	}
	if serverInfo, ok := resultMap["serverInfo"].(map[string]interface{}); ok {
		if name, ok := serverInfo["name"].(string); ok {
			initResult.ServerInfo.Name = name
		}
		if version, ok := serverInfo["version"].(string); ok {
			initResult.ServerInfo.Version = version
		}
	}

	return initResult, nil
}

// IsInitialized returns whether the client is initialized
func (c *McpClient) IsInitialized() bool {
	return c.initialized
}

// ListTools lists available tools
func (c *McpClient) ListTools(ctx context.Context) ([]Tool, error) {
	result, err := c.sendRequest(ctx, "tools/list", map[string]interface{}{})
	if err != nil {
		return nil, err
	}

	resultMap, ok := result.(map[string]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid tools/list result"}}
	}

	toolsRaw, ok := resultMap["tools"].([]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid tools array"}}
	}

	tools := make([]Tool, 0, len(toolsRaw))
	for _, toolRaw := range toolsRaw {
		toolMap, ok := toolRaw.(map[string]interface{})
		if !ok {
			continue
		}

		tool := Tool{}
		if name, ok := toolMap["name"].(string); ok {
			tool.Name = name
		}
		if description, ok := toolMap["description"].(string); ok {
			tool.Description = description
		}
		if inputSchema, ok := toolMap["inputSchema"].(map[string]interface{}); ok {
			tool.InputSchema = inputSchema
		}
		tools = append(tools, tool)
	}

	return tools, nil
}

// CallTool calls a tool with the given arguments
func (c *McpClient) CallTool(ctx context.Context, name string, args map[string]interface{}) (*ToolCallResult, error) {
	params := map[string]interface{}{
		"name":      name,
		"arguments": args,
	}

	result, err := c.sendRequest(ctx, "tools/call", params)
	if err != nil {
		return nil, err
	}

	resultMap, ok := result.(map[string]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid tools/call result"}}
	}

	toolResult := &ToolCallResult{}
	if isError, ok := resultMap["isError"].(bool); ok {
		toolResult.IsError = isError
	}

	if contentRaw, ok := resultMap["content"].([]interface{}); ok {
		content := make([]Content, 0, len(contentRaw))
		for _, contentItemRaw := range contentRaw {
			contentItemMap, ok := contentItemRaw.(map[string]interface{})
			if !ok {
				continue
			}

			contentItem := Content{}
			if contentType, ok := contentItemMap["type"].(string); ok {
				contentItem.Type = contentType
			}
			if text, ok := contentItemMap["text"].(string); ok {
				contentItem.Text = text
			}
			if data, ok := contentItemMap["data"].(string); ok {
				contentItem.Data = data
			}
			if mimeType, ok := contentItemMap["mimeType"].(string); ok {
				contentItem.MimeType = mimeType
			}
			content = append(content, contentItem)
		}
		toolResult.Content = content
	}

	if toolResult.IsError {
		errorMsg := ""
		for _, content := range toolResult.Content {
			if content.Type == "text" {
				errorMsg += content.Text + "\n"
			}
		}
		return nil, &ToolExecutionError{McpError{Message: errorMsg}}
	}

	return toolResult, nil
}

// ListResources lists available resources
func (c *McpClient) ListResources(ctx context.Context) ([]Resource, error) {
	result, err := c.sendRequest(ctx, "resources/list", map[string]interface{}{})
	if err != nil {
		return nil, err
	}

	resultMap, ok := result.(map[string]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid resources/list result"}}
	}

	resourcesRaw, ok := resultMap["resources"].([]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid resources array"}}
	}

	resources := make([]Resource, 0, len(resourcesRaw))
	for _, resourceRaw := range resourcesRaw {
		resourceMap, ok := resourceRaw.(map[string]interface{})
		if !ok {
			continue
		}

		resource := Resource{}
		if uri, ok := resourceMap["uri"].(string); ok {
			resource.URI = uri
		}
		if name, ok := resourceMap["name"].(string); ok {
			resource.Name = name
		}
		if description, ok := resourceMap["description"].(string); ok {
			resource.Description = &description
		}
		if mimeType, ok := resourceMap["mimeType"].(string); ok {
			resource.MimeType = &mimeType
		}
		resources = append(resources, resource)
	}

	return resources, nil
}

// ReadResource reads a resource by URI
func (c *McpClient) ReadResource(ctx context.Context, uri string) (*ResourceReadResult, error) {
	params := map[string]interface{}{
		"uri": uri,
	}

	result, err := c.sendRequest(ctx, "resources/read", params)
	if err != nil {
		return nil, err
	}

	resultMap, ok := result.(map[string]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid resources/read result"}}
	}

	readResult := &ResourceReadResult{}
	if contentsRaw, ok := resultMap["contents"].([]interface{}); ok {
		contents := make([]ResourceContent, 0, len(contentsRaw))
		for _, contentRaw := range contentsRaw {
			contentMap, ok := contentRaw.(map[string]interface{})
			if !ok {
				continue
			}

			content := ResourceContent{}
			if uri, ok := contentMap["uri"].(string); ok {
				content.URI = uri
			}
			if mimeType, ok := contentMap["mimeType"].(string); ok {
				content.MimeType = &mimeType
			}
			if text, ok := contentMap["text"].(string); ok {
				content.Text = &text
			}
			contents = append(contents, content)
		}
		readResult.Contents = contents
	}

	return readResult, nil
}

// ListPrompts lists available prompts
func (c *McpClient) ListPrompts(ctx context.Context) ([]Prompt, error) {
	result, err := c.sendRequest(ctx, "prompts/list", map[string]interface{}{})
	if err != nil {
		return nil, err
	}

	resultMap, ok := result.(map[string]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid prompts/list result"}}
	}

	promptsRaw, ok := resultMap["prompts"].([]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid prompts array"}}
	}

	prompts := make([]Prompt, 0, len(promptsRaw))
	for _, promptRaw := range promptsRaw {
		promptMap, ok := promptRaw.(map[string]interface{})
		if !ok {
			continue
		}

		prompt := Prompt{}
		if name, ok := promptMap["name"].(string); ok {
			prompt.Name = name
		}
		if description, ok := promptMap["description"].(string); ok {
			prompt.Description = &description
		}
		prompts = append(prompts, prompt)
	}

	return prompts, nil
}

// GetPrompt gets a prompt with the given arguments
func (c *McpClient) GetPrompt(ctx context.Context, name string, args map[string]interface{}) (*PromptGetResult, error) {
	params := map[string]interface{}{
		"name":      name,
		"arguments": args,
	}

	result, err := c.sendRequest(ctx, "prompts/get", params)
	if err != nil {
		return nil, err
	}

	resultMap, ok := result.(map[string]interface{})
	if !ok {
		return nil, &ServerError{McpError{Message: "invalid prompts/get result"}}
	}

	getResult := &PromptGetResult{}
	if description, ok := resultMap["description"].(string); ok {
		getResult.Description = description
	}

	return getResult, nil
}

// Ping pings the server
func (c *McpClient) Ping(ctx context.Context) error {
	_, err := c.sendRequest(ctx, "ping", map[string]interface{}{})
	return err
}

// SetLogLevel sets the logging level
func (c *McpClient) SetLogLevel(ctx context.Context, level string) error {
	params := map[string]interface{}{
		"level": level,
	}
	_, err := c.sendRequest(ctx, "logging/setLevel", params)
	return err
}

// Close closes the client connection
func (c *McpClient) Close() error {
	return c.transport.Close()
}
