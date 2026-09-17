# VELOCITY-MCP Plugin Marketplace

The plugin marketplace provides a centralized registry for discovering, installing, and managing plugins for VELOCITY-MCP.

## Features

- **Plugin Discovery**: Search and browse available plugins
- **One-Click Installation**: Install plugins with a single API call (downloads the archive and verifies its SHA-256 checksum)
- **Version Management**: Track plugin versions and check/apply updates
- **Ratings and Reviews**: Submit a 1-5 star review per plugin
- **Statistics**: Track downloads, ratings, and installed plugins

## API Endpoints

All marketplace endpoints are served under `/v1/marketplace/` and, like every route except the top-level `/health`, require an `Authorization: Bearer <key>` header when an API key is configured (`[http].api_key` or `VELOCITY_API_KEY`). There are no marketplace CLI subcommands — this HTTP API is the only interface. Enabled/disabled state is exposed on each installed plugin record; toggling it is a library-level call (`Marketplace::set_enabled`), not an HTTP route.

### List Plugins

```bash
GET /v1/marketplace/plugins?query=search+text&tags=tag1,tag2&author=author&verified_only=true&sort_by=downloads&limit=20&offset=0
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
GET /v1/marketplace/plugins/:id
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
  "min_velocity_version": "3.2.0",
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
POST /v1/marketplace/install/:id
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
DELETE /v1/marketplace/install/:id
```

**Response:** `200 OK`

### List Installed Plugins

```bash
GET /v1/marketplace/installed
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

### Submit a Review

```bash
POST /v1/marketplace/plugins/:id/review
```

**Request body:**
```json
{
  "reviewer": "author-or-user",
  "rating": 5,
  "comment": "Works well"
}
```

`rating` must be between 1 and 5. **Response:** `201 Created`; the plugin's `rating` and `rating_count` are recomputed from all stored reviews.

### Check for Updates

```bash
GET /v1/marketplace/updates
```

**Response:** array of installed plugins that have a newer version in the index (field names from `PluginUpdate`):

```json
[
  {
    "plugin_id": "author.plugin-name",
    "current_version": "1.0.0",
    "latest_version": "1.1.0",
    "download_url": "https://example.com/plugin-1.1.0.zip"
  }
]
```

### Apply an Update

```bash
POST /v1/marketplace/update/:id
```

**Response:** the refreshed `InstalledPlugin` record.

### Get Marketplace Statistics

```bash
GET /v1/marketplace/stats
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

### WASM Plugin Manifest

For WASM-based plugins, use the `wasm` executor type with a supported language:

```json
{
  "name": "my-wasm-plugin",
  "version": "1.0.0",
  "tools": [
    {
      "name": "my_wasm_tool",
      "description": "A tool running in WebAssembly",
      "inputSchema": {
        "type": "object",
        "properties": {
          "text": {
            "type": "string",
            "description": "Input text"
          }
        },
        "required": ["text"]
      },
      "executor": {
        "executor_type": "wasm",
        "language": "javascript",
        "source": "function my_tool(args) { return { result: args.text.toUpperCase() }; }",
        "timeout": 5
      }
    }
  ]
}
```

Supported WASM languages (13 total — 7 production runtimes + 6 tree-walk interpreters, one per `[wasm_runtimes.<lang>]` key):

- **Production runtimes**: `javascript`, `typescript` (via QuickJS), `python` (MicroPython), `ruby` (mruby), `lua`, `go` (TinyGo), `rust`
- **Tree-walk interpreters** (built on a shared C core): `php`, `csharp`, `java`, `r`, `julia`, `perl`

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
  "min_velocity_version": "3.2.0",
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
import os
import requests

BASE = "http://localhost:3000/v1/marketplace"
HEADERS = {"Authorization": f"Bearer {os.environ['VELOCITY_API_KEY']}"}

# List plugins
response = requests.get(f"{BASE}/plugins", headers=HEADERS)
plugins = response.json()

# Install plugin
response = requests.post(f"{BASE}/install/author.plugin-name", headers=HEADERS)
installed = response.json()

# List installed
response = requests.get(f"{BASE}/installed", headers=HEADERS)
installed = response.json()
```

### JavaScript

```javascript
const base = "http://localhost:3000/v1/marketplace";
const headers = { Authorization: `Bearer ${process.env.VELOCITY_API_KEY}` };

// List plugins
const response = await fetch(`${base}/plugins`, { headers });
const plugins = await response.json();

// Install plugin
const install = await fetch(`${base}/install/author.plugin-name`, {
  method: "POST",
  headers
});
const installed = await install.json();
```

### cURL

```bash
AUTH="Authorization: Bearer $VELOCITY_API_KEY"

# List plugins
curl -H "$AUTH" http://localhost:3000/v1/marketplace/plugins

# Get plugin details
curl -H "$AUTH" http://localhost:3000/v1/marketplace/plugins/author.plugin-name

# Install plugin
curl -X POST -H "$AUTH" http://localhost:3000/v1/marketplace/install/author.plugin-name

# Uninstall plugin
curl -X DELETE -H "$AUTH" http://localhost:3000/v1/marketplace/install/author.plugin-name

# List installed
curl -H "$AUTH" http://localhost:3000/v1/marketplace/installed

# Check for updates / apply one
curl -H "$AUTH" http://localhost:3000/v1/marketplace/updates
curl -X POST -H "$AUTH" http://localhost:3000/v1/marketplace/update/author.plugin-name

# Submit a review
curl -X POST -H "$AUTH" -H "Content-Type: application/json" \
  -d '{"reviewer":"me","rating":5,"comment":"works"}' \
  http://localhost:3000/v1/marketplace/plugins/author.plugin-name/review

# Get statistics
curl -H "$AUTH" http://localhost:3000/v1/marketplace/stats
```

## Future Enhancements

Already shipped: ratings/reviews (`POST /v1/marketplace/plugins/:id/review`) and update checking/applying (`GET /v1/marketplace/updates`, `POST /v1/marketplace/update/:id`).

Still to come:

- Plugin dependency resolution (a `dependencies` list is stored in the manifest but not resolved or installed)
- Plugin marketplace web UI
- Plugin signing (installation verifies a SHA-256 checksum only; there is no publisher signature scheme)
- Plugin analytics and metrics
