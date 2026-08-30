# VELOCITY-MCP Plugin Marketplace

The plugin marketplace provides a centralized registry for discovering, installing, and managing plugins for VELOCITY-MCP.

## Features

- **Plugin Discovery**: Search and browse available plugins
- **One-Click Installation**: Install plugins with a single API call
- **Version Management**: Track plugin versions and updates
- **Plugin Management**: Enable/disable installed plugins
- **Statistics**: Track downloads, ratings, and usage

## API Endpoints

All marketplace endpoints are available under `/marketplace/` and require authentication if enabled.

### List Plugins

```bash
GET /marketplace/plugins?query=search+text&tags=tag1,tag2&author=author&verified_only=true&sort_by=downloads&limit=20&offset=0
```

**Query Parameters:**
- `query` (string): Search text (matches name, description, tags)
- `tags` (array): Filter by tags
- `author` (string): Filter by author
- `verified_only` (boolean): Show only verified plugins
- `sort_by` (string): Sort by "downloads", "rating", or "updated_at" (default: "downloads")
- `limit` (integer): Maximum results (default: 20)
- `offset` (integer): Pagination offset (default: 0)

**Response:**
```json
{
  "plugins": [
    {
      "id": "author.plugin-name",
      "name": "Plugin Name",
      "version": "1.0.0",
      "author": "Author Name",
      "description": "Short description",
      "tags": ["tag1", "tag2"],
      "downloads": 1000,
      "rating": 4.5,
      "verified": true
    }
  ],
  "total": 100,
  "offset": 0,
  "limit": 20
}
```

### Get Plugin Details

```bash
GET /marketplace/plugins/:id
```

**Response:**
```json
{
  "id": "author.plugin-name",
  "name": "Plugin Name",
  "version": "1.0.0",
  "author": "Author Name",
  "description": "Short description",
  "documentation": "# Full documentation in markdown...",
  "tags": ["tag1", "tag2"],
  "download_url": "https://example.com/plugin.zip",
  "checksum": "sha256:...",
  "min_velocity_version": "3.0.0",
  "dependencies": ["other.plugin"],
  "downloads": 1000,
  "rating": 4.5,
  "rating_count": 50,
  "created_at": "2026-01-01T00:00:00Z",
  "updated_at": "2026-01-15T00:00:00Z",
  "verified": true
}
```

### Install Plugin

```bash
POST /marketplace/install/:id
```

**Response:**
```json
{
  "metadata": { ... },
  "install_path": "/path/to/plugins/author.plugin-name",
  "installed_at": "2026-01-20T00:00:00Z",
  "enabled": true
}
```

### Uninstall Plugin

```bash
DELETE /marketplace/install/:id
```

**Response:** `200 OK`

### List Installed Plugins

```bash
GET /marketplace/installed
```

**Response:**
```json
[
  {
    "metadata": { ... },
    "install_path": "/path/to/plugins/author.plugin-name",
    "installed_at": "2026-01-20T00:00:00Z",
    "enabled": true
  }
]
```

### Get Marketplace Statistics

```bash
GET /marketplace/stats
```

**Response:**
```json
{
  "total_plugins": 100,
  "installed_plugins": 5,
  "verified_plugins": 50,
  "total_downloads": 10000
}
```

## Creating Plugins

### Plugin Manifest

Create a `manifest.json` file in your plugin directory:

```json
{
  "name": "my-plugin",
  "version": "1.0.0",
  "tools": [
    {
      "name": "my_tool",
      "description": "A custom tool",
      "inputSchema": {
        "type": "object",
        "properties": {
          "param1": {
            "type": "string",
            "description": "Parameter description"
          }
        },
        "required": ["param1"]
      },
      "executor": {
        "executor_type": "process",
        "command": "python",
        "args": ["my_tool.py", "--param1", "{{param1}}"],
        "timeout": 30
      }
    }
  ]
}
```

### Marketplace Metadata

For marketplace listing, create a `marketplace.json` file:

```json
{
  "id": "author.my-plugin",
  "name": "My Plugin",
  "version": "1.0.0",
  "author": "Your Name",
  "description": "Short description for listing",
  "documentation": "# Full documentation...",
  "tags": ["utility", "example"],
  "download_url": "https://github.com/author/my-plugin/releases/download/v1.0.0/plugin.zip",
  "checksum": "sha256:abc123...",
  "min_velocity_version": "3.0.0",
  "dependencies": [],
  "verified": false
}
```

### Plugin Structure

```
my-plugin/
├── manifest.json       # Plugin manifest (required)
├── marketplace.json    # Marketplace metadata (for marketplace listing)
├── my_tool.py          # Tool implementation
└── README.md           # Documentation
```

## Submitting to Marketplace

To submit your plugin to the official marketplace:

1. Ensure your plugin follows the manifest format
2. Create comprehensive documentation
3. Test your plugin thoroughly
4. Submit a pull request to the marketplace repository (coming soon)

## Plugin Security

- All plugins are executed in isolated processes
- Plugins cannot access the VELOCITY-MCP server's internal state
- Plugins are subject to the same sandbox restrictions as other tools
- Verified plugins undergo additional security review

## Examples

### Python Plugin Example

```python
#!/usr/bin/env python3
import sys
import json

def my_tool(param1):
    """Tool implementation"""
    return {"result": f"Processed: {param1}"}

if __name__ == "__main__":
    # Read arguments from stdin
    args = json.loads(sys.stdin.read())
    
    # Execute tool
    result = my_tool(args["param1"])
    
    # Output result as JSON
    print(json.dumps(result))
```

### Bash Plugin Example

```bash
#!/bin/bash
# my_tool.sh

# Read arguments
PARAM1="$1"

# Execute tool
echo "Processed: $PARAM1"
```

## Troubleshooting

### Plugin Not Loading

1. Check that `manifest.json` is valid JSON
2. Verify the plugin directory is in the configured `plugin_dir`
3. Check server logs for error messages
4. Ensure tool names are unique

### Installation Failed

1. Verify the download URL is accessible
2. Check the checksum matches
3. Ensure sufficient disk space
4. Check server logs for detailed error

### Tool Execution Failed

1. Verify the command exists and is executable
2. Check tool permissions
3. Review tool logs for errors
4. Test the tool manually outside of VELOCITY-MCP

## API Client Examples

### Python

```python
import requests

# List plugins
response = requests.get("http://localhost:3000/marketplace/plugins")
plugins = response.json()

# Install plugin
response = requests.post("http://localhost:3000/marketplace/install/author.plugin-name")
installed = response.json()

# List installed
response = requests.get("http://localhost:3000/marketplace/installed")
installed = response.json()
```

### JavaScript

```javascript
// List plugins
const response = await fetch("http://localhost:3000/marketplace/plugins");
const plugins = await response.json();

// Install plugin
const response = await fetch("http://localhost:3000/marketplace/install/author.plugin-name", {
  method: "POST"
});
const installed = await response.json();
```

### cURL

```bash
# List plugins
curl http://localhost:3000/marketplace/plugins

# Get plugin details
curl http://localhost:3000/marketplace/plugins/author.plugin-name

# Install plugin
curl -X POST http://localhost:3000/marketplace/install/author.plugin-name

# Uninstall plugin
curl -X DELETE http://localhost:3000/marketplace/install/author.plugin-name

# List installed
curl http://localhost:3000/marketplace/installed

# Get statistics
curl http://localhost:3000/marketplace/stats
```

## Future Enhancements

- Plugin ratings and reviews
- Automatic updates
- Plugin dependencies resolution
- Plugin marketplace web UI
- Plugin signing and verification
- Plugin analytics and metrics
