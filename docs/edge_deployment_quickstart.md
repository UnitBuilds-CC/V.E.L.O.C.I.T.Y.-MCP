# Wasmer Edge Deployment Quickstart

## Prerequisites

1. **Wasmer CLI installed** (already done - v7.4.1 at `~/.wasmer/bin`)
2. **Logged in to Wasmer**: Run `wasmer login` or use `C:\Users\ian\.wasmer\login.bat`
3. **Free tier account**: Configured for 128MB memory, 5M instructions/request

## Deploy in 3 Steps

### Step 1: Login (one-time)
```bash
C:\Users\ian\.wasmer\login.bat
```
This opens your browser to authenticate with Wasmer.

### Step 2: Build & Deploy
```bash
deploy-edge.bat
```

Or manually:
```bash
cargo build --target wasm32-wasip1 --release --bin velocity-edge
wasmer deploy
```

### Step 3: Test Your Endpoint
After deployment, you'll get a URL like `https://velocity-mcp-edge.wasmer.app`

Test with curl:
```bash
curl https://velocity-mcp-edge.wasmer.app \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}'
```

## Free Tier Limits

- **Memory**: 128MB (optimized from default 512MB)
- **Instructions**: 5M per request (~50ms compute budget)
- **Instances**: 0-3 (auto-scaling, scales to zero when idle)
- **Storage**: No persistent volumes on free tier
- **Bandwidth**: Standard CDN distribution

## What's Included

The WASM Edge deployment provides:
- ✅ HTTP/SSE transport (JSON-RPC over HTTP)
- ✅ All 13 language runtimes (QuickJS, Lua, MicroPython, etc.)
- ✅ Dynamic tool registration
- ✅ Metering middleware (instruction counting)
- ✅ Module compilation caching

Not available on Edge:
- ❌ NDA-native shared memory transport
- ❌ Process spawning (shell_exec)
- ❌ Local filesystem access beyond temp storage
- ❌ Custom TCP/UDP sockets

## Monitoring

View logs and metrics:
```bash
wasmer edge list              # List deployments
wasmer edge logs <app-name>   # View logs
wasmer edge metrics <app-name> # View metrics
```

## Cost Estimation

Free tier includes ~1M invocations/month. Beyond that:
- Compute: Pay per instruction executed (metered)
- Bandwidth: Standard egress rates
- Storage: If using volumes (not included in this deployment)

For typical MCP usage (100-1000 requests/day), you should stay within free tier limits.

## Troubleshooting

**Build fails**: Ensure `wasm32-wasip1` target is installed:
```bash
rustup target add wasm32-wasip1
```

**Deployment rejected**: Check file size (<50MB) and memory config (128MB)

**Timeout errors**: Reduce instruction limit or optimize tool execution time

**Out of memory**: The module cache may be too large. Consider disabling it for Edge:
```rust
// In velocity_edge.rs, pass instruction_limit: None to disable metering overhead
```

## Next Steps

After successful deployment:
1. Share the endpoint URL with your MCP clients
2. Monitor usage via `wasmer edge metrics`
3. Upgrade plan if you exceed free tier limits
4. Consider hybrid architecture: native binary for local use, Edge for remote access
