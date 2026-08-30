# VELOCITY-MCP Deployment Guide

Production deployment guide for VELOCITY-MCP server v3.0.0.

## Table of Contents

- [System Requirements](#system-requirements)
- [Installation](#installation)
- [Production Configuration](#production-configuration)
- [Running as a Service](#running-as-a-service)
- [TLS/HTTPS Setup](#tlshttps-setup)
- [Monitoring and Logging](#monitoring-and-logging)
- [Security Hardening](#security-hardening)
- [Scaling](#scaling)
- [Backup and Recovery](#backup-and-recovery)
- [Troubleshooting](#troubleshooting)

---

## System Requirements

### Minimum Requirements

- **CPU:** 2 cores
- **RAM:** 512 MB
- **Disk:** 100 MB
- **OS:** Linux (kernel 4.15+), macOS 10.15+, Windows 10+

### Recommended Requirements

- **CPU:** 4+ cores
- **RAM:** 2+ GB
- **Disk:** 1+ GB (for logs and NDA documents)
- **Network:** 100+ Mbps for HTTP mode

### Platform-Specific Notes

**Linux:**
- systemd required for service management
- Recommended: Ubuntu 20.04+, Debian 11+, RHEL 8+

**macOS:**
- launchd for service management
- Recommended: macOS 12+ (Monterey)

**Windows:**
- Windows Service or Task Scheduler
- Recommended: Windows Server 2019+, Windows 10+

---

## Installation

### Method 1: Pre-built Binaries (Recommended)

**Linux (x86_64):**
```bash
# Download latest release
curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp_linux_x86_64 -o velocity_mcp

# Make executable
chmod +x velocity_mcp

# Move to system path
sudo mv velocity_mcp /usr/local/bin/

# Verify installation
velocity_mcp --version
```

**macOS (x86_64):**
```bash
curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp_macos_x86_64 -o velocity_mcp
chmod +x velocity_mcp
sudo mv velocity_mcp /usr/local/bin/
velocity_mcp --version
```

**macOS (ARM64/Apple Silicon):**
```bash
curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp_macos_arm64 -o velocity_mcp
chmod +x velocity_mcp
sudo mv velocity_mcp /usr/local/bin/
velocity_mcp --version
```

**Windows (x86_64):**
```powershell
# Download using PowerShell
Invoke-WebRequest -Uri "https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp_windows_x86_64.exe" -OutFile "velocity_mcp.exe"

# Move to system path
Move-Item -Path "velocity_mcp.exe" -Destination "C:\Program Files\velocity_mcp.exe"

# Verify installation
& "C:\Program Files\velocity_mcp.exe" --version
```

### Method 2: Build from Source

**Prerequisites:**
- Rust 1.75+ (https://rustup.rs)
- Git
- Build tools (gcc, make)

**Build steps:**
```bash
# Clone repository
git clone https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP.git
cd V.E.L.O.C.I.T.Y.-MCP

# Build release binary
cargo build --release --features http,database,oauth2

# Binary location
ls -lh target/release/velocity_mcp

# Install to system path
sudo cp target/release/velocity_mcp /usr/local/bin/
```

### Method 3: Docker

**Dockerfile:**
```dockerfile
FROM rust:1.75 as builder
WORKDIR /build
COPY . .
RUN cargo build --release --features http,database,oauth2

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/velocity_mcp /usr/local/bin/
EXPOSE 3000
CMD ["velocity_mcp", "--mode", "http", "--addr", "0.0.0.0:3000"]
```

**Build and run:**
```bash
docker build -t velocity-mcp .
docker run -d -p 3000:3000 --name velocity-mcp velocity-mcp
```

---

## Production Configuration

### Configuration File

Create `/etc/velocity-mcp/config.toml`:

```toml
[server]
mode = "http"
version = "3.0.0"

[http]
addr = "0.0.0.0:3000"
api_key = "${VELOCITY_API_KEY}"  # Use environment variable
rate_limit = 100
rate_burst = 500
max_body_size = 10485760  # 10MB
cors_origins = ["https://your-domain.com"]

[http.tls]
enabled = true
cert = "/etc/velocity-mcp/tls/cert.pem"
key = "/etc/velocity-mcp/tls/key.pem"

[logging]
level = "info"
format = "json"
output = "/var/log/velocity-mcp/server.log"

[security]
max_sessions = 10000
session_timeout = 1800  # 30 minutes
enable_audit_log = true
audit_log_path = "/var/log/velocity-mcp/audit.log"

[performance]
enable_metrics = true
metrics_interval = 60
```

### Environment Variables

Create `/etc/velocity-mcp/env`:

```bash
# API Key (generate with: openssl rand -hex 32)
VELOCITY_API_KEY=your-secure-api-key-here

# Logging
RUST_LOG=info

# Optional: Database path
VELOCITY_DATABASE_PATH=/var/lib/velocity-mcp/data.db

# Optional: Custom temp directory
VELOCITY_TEMP_DIR=/var/lib/velocity-mcp/tmp
```

Set permissions:
```bash
sudo chmod 600 /etc/velocity-mcp/env
sudo chown velocity-mcp:velocity-mcp /etc/velocity-mcp/env
```

### Directory Structure

```bash
# Create directories
sudo mkdir -p /etc/velocity-mcp/tls
sudo mkdir -p /var/log/velocity-mcp
sudo mkdir -p /var/lib/velocity-mcp/tmp
sudo mkdir -p /var/lib/velocity-mcp/data

# Set permissions
sudo chown -R velocity-mcp:velocity-mcp /var/log/velocity-mcp
sudo chown -R velocity-mcp:velocity-mcp /var/lib/velocity-mcp
sudo chmod 750 /var/log/velocity-mcp
sudo chmod 750 /var/lib/velocity-mcp
```

---

## Running as a Service

### Linux (systemd)

**Create service user:**
```bash
sudo useradd -r -s /bin/false -d /var/lib/velocity-mcp velocity-mcp
```

**Create systemd service file** `/etc/systemd/system/velocity-mcp.service`:

```ini
[Unit]
Description=VELOCITY-MCP Server
After=network.target

[Service]
Type=simple
User=velocity-mcp
Group=velocity-mcp

# Load environment variables
EnvironmentFile=/etc/velocity-mcp/env

# Start command
ExecStart=/usr/local/bin/velocity_mcp --config /etc/velocity-mcp/config.toml

# Restart policy
Restart=always
RestartSec=10

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true

# Resource limits
LimitNOFILE=65536
LimitNPROC=4096

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=velocity-mcp

[Install]
WantedBy=multi-user.target
```

**Enable and start service:**
```bash
# Reload systemd
sudo systemctl daemon-reload

# Enable service
sudo systemctl enable velocity-mcp

# Start service
sudo systemctl start velocity-mcp

# Check status
sudo systemctl status velocity-mcp

# View logs
sudo journalctl -u velocity-mcp -f
```

### macOS (launchd)

**Create plist file** `/Library/LaunchDaemons/com.velocity-mcp.server.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.velocity-mcp.server</string>
    
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/velocity_mcp</string>
        <string>--config</string>
        <string>/etc/velocity-mcp/config.toml</string>
    </array>
    
    <key>RunAtLoad</key>
    <true/>
    
    <key>KeepAlive</key>
    <true/>
    
    <key>StandardOutPath</key>
    <string>/var/log/velocity-mcp/server.log</string>
    
    <key>StandardErrorPath</key>
    <string>/var/log/velocity-mcp/error.log</string>
    
    <key>EnvironmentVariables</key>
    <dict>
        <key>VELOCITY_API_KEY</key>
        <string>your-secure-api-key-here</string>
    </dict>
    
    <key>UserName</key>
    <string>_velocity-mcp</string>
</dict>
</plist>
```

**Load and start:**
```bash
sudo launchctl load /Library/LaunchDaemons/com.velocity-mcp.server.plist
sudo launchctl start com.velocity-mcp.server
```

### Windows (Service)

**Using NSSM (Non-Sucking Service Manager):**

```powershell
# Download NSSM
Invoke-WebRequest -Uri "https://nssm.cc/release/nssm-2.24.zip" -OutFile "nssm.zip"
Expand-Archive nssm.zip

# Install service
.\nssm-2.24\win64\nssm install velocity-mcp "C:\Program Files\velocity_mcp.exe"
.\nssm-2.24\win64\nssm set velocity-mcp AppParameters "--mode http --addr 0.0.0.0:3000"
.\nssm-2.24\win64\nssm set velocity-mcp AppDirectory "C:\Program Files"
.\nssm-2.24\win64\nssm set velocity-mcp AppEnvironmentExtra "VELOCITY_API_KEY=your-secure-api-key-here"
.\nssm-2.24\win64\nssm set velocity-mcp AppStdout "C:\ProgramData\velocity-mcp\logs\stdout.log"
.\nssm-2.24\win64\nssm set velocity-mcp AppStderr "C:\ProgramData\velocity-mcp\logs\stderr.log"

# Start service
.\nssm-2.24\win64\nssm start velocity-mcp
```

---

## TLS/HTTPS Setup

### Generate Self-Signed Certificate (Testing)

```bash
# Create directory
sudo mkdir -p /etc/velocity-mcp/tls

# Generate certificate
sudo openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
  -keyout /etc/velocity-mcp/tls/key.pem \
  -out /etc/velocity-mcp/tls/cert.pem \
  -subj "/C=US/ST=State/L=City/O=Organization/CN=localhost"

# Set permissions
sudo chmod 600 /etc/velocity-mcp/tls/*.pem
sudo chown velocity-mcp:velocity-mcp /etc/velocity-mcp/tls/*.pem
```

### Let's Encrypt Certificate (Production)

**Using certbot:**
```bash
# Install certbot
sudo apt-get install certbot

# Generate certificate
sudo certbot certonly --standalone -d mcp.your-domain.com

# Certificates location
# /etc/letsencrypt/live/mcp.your-domain.com/fullchain.pem
# /etc/letsencrypt/live/mcp.your-domain.com/privkey.pem

# Copy to velocity-mcp directory
sudo cp /etc/letsencrypt/live/mcp.your-domain.com/fullchain.pem /etc/velocity-mcp/tls/cert.pem
sudo cp /etc/letsencrypt/live/mcp.your-domain.com/privkey.pem /etc/velocity-mcp/tls/key.pem

# Set permissions
sudo chmod 600 /etc/velocity-mcp/tls/*.pem
sudo chown velocity-mcp:velocity-mcp /etc/velocity-mcp/tls/*.pem
```

**Auto-renewal cron job:**
```bash
sudo crontab -e

# Add line:
0 0 1 * * certbot renew --quiet && cp /etc/letsencrypt/live/mcp.your-domain.com/fullchain.pem /etc/velocity-mcp/tls/cert.pem && cp /etc/letsencrypt/live/mcp.your-domain.com/privkey.pem /etc/velocity-mcp/tls/key.pem && systemctl restart velocity-mcp
```

### Update Configuration

Edit `/etc/velocity-mcp/config.toml`:

```toml
[http.tls]
enabled = true
cert = "/etc/velocity-mcp/tls/cert.pem"
key = "/etc/velocity-mcp/tls/key.pem"
```

Restart service:
```bash
sudo systemctl restart velocity-mcp
```

---

## Monitoring and Logging

### Log Rotation

**Create logrotate config** `/etc/logrotate.d/velocity-mcp`:

```
/var/log/velocity-mcp/*.log {
    daily
    rotate 30
    compress
    delaycompress
    missingok
    notifempty
    create 0640 velocity-mcp velocity-mcp
    postrotate
        systemctl restart velocity-mcp > /dev/null 2>&1 || true
    endscript
}
```

### Prometheus Metrics

**Enable metrics endpoint:**

```toml
[performance]
enable_metrics = true
metrics_interval = 60
```

**Access metrics:**
```bash
curl http://localhost:3000/metrics
```

**Prometheus config** `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'velocity-mcp'
    scrape_interval: 15s
    metrics_path: '/metrics'
    static_configs:
      - targets: ['localhost:3000']
```

### Grafana Dashboard

**Import dashboard:**
1. Open Grafana
2. Go to Dashboards → Import
3. Use dashboard ID: 1860 (or create custom)
4. Select Prometheus data source

**Key metrics to monitor:**
- `velocity_mcp_requests_total` - Total requests
- `velocity_mcp_request_duration_seconds` - Request latency
- `velocity_mcp_active_sessions` - Active sessions
- `velocity_mcp_errors_total` - Error count
- `velocity_mcp_rate_limit_hits_total` - Rate limit hits

### Health Checks

**HTTP health check:**
```bash
curl -f http://localhost:3000/health || exit 1
```

**Kubernetes liveness probe:**
```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 3000
  initialDelaySeconds: 10
  periodSeconds: 10
```

---

## Security Hardening

### Firewall Configuration

**Linux (iptables):**
```bash
# Allow HTTP/HTTPS
sudo iptables -A INPUT -p tcp --dport 3000 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 443 -j ACCEPT

# Save rules
sudo iptables-save > /etc/iptables/rules.v4
```

**Linux (firewalld):**
```bash
sudo firewall-cmd --permanent --add-port=3000/tcp
sudo firewall-cmd --permanent --add-port=443/tcp
sudo firewall-cmd --reload
```

**Windows (PowerShell):**
```powershell
New-NetFirewallRule -DisplayName "VELOCITY-MCP" -Direction Inbound -Port 3000 -Protocol TCP -Action Allow
```

### API Key Rotation

**Generate new API key:**
```bash
openssl rand -hex 32
```

**Update configuration:**
```bash
sudo nano /etc/velocity-mcp/env
# Update VELOCITY_API_KEY

# Restart service
sudo systemctl restart velocity-mcp
```

### Audit Logging

**Enable audit logging:**
```toml
[security]
enable_audit_log = true
audit_log_path = "/var/log/velocity-mcp/audit.log"
```

**Monitor audit log:**
```bash
sudo tail -f /var/log/velocity-mcp/audit.log
```

**Audit log format:**
```json
{
  "timestamp": "2026-08-30T01:00:00Z",
  "event": "tool_call",
  "tool": "file_read",
  "session": "session-abc123",
  "outcome": "success",
  "duration_ms": 15
}
```

### Rate Limiting

**Configure rate limits:**
```toml
[http]
rate_limit = 100      # Requests per second
rate_burst = 500      # Burst capacity
```

**Monitor rate limit hits:**
```bash
curl http://localhost:3000/metrics | grep rate_limit
```

---

## Scaling

### Vertical Scaling

**Increase resource limits:**

Edit `/etc/systemd/system/velocity-mcp.service`:

```ini
[Service]
LimitNOFILE=65536
LimitNPROC=4096
MemoryMax=2G
CPUQuota=200%
```

Reload and restart:
```bash
sudo systemctl daemon-reload
sudo systemctl restart velocity-mcp
```

### Horizontal Scaling

**Load balancer configuration (nginx):**

```nginx
upstream velocity_mcp {
    server 10.0.0.1:3000;
    server 10.0.0.2:3000;
    server 10.0.0.3:3000;
}

server {
    listen 443 ssl;
    server_name mcp.your-domain.com;

    ssl_certificate /etc/letsencrypt/live/mcp.your-domain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/mcp.your-domain.com/privkey.pem;

    location / {
        proxy_pass http://velocity_mcp;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

### Database Scaling

**SQLite optimization:**
```toml
[database]
path = "/var/lib/velocity-mcp/data.db"
pool_size = 10
cache_size = 10000
```

**PostgreSQL (future):**
```toml
[database]
type = "postgresql"
host = "localhost"
port = 5432
database = "velocity_mcp"
user = "velocity"
password = "${DB_PASSWORD}"
```

---

## Backup and Recovery

### Backup Strategy

**Automated backup script** `/usr/local/bin/velocity-mcp-backup.sh`:

```bash
#!/bin/bash
set -e

BACKUP_DIR="/var/backups/velocity-mcp"
DATE=$(date +%Y%m%d_%H%M%S)

# Create backup directory
mkdir -p $BACKUP_DIR

# Backup configuration
tar -czf $BACKUP_DIR/config_$DATE.tar.gz /etc/velocity-mcp/

# Backup database (if using)
if [ -f /var/lib/velocity-mcp/data.db ]; then
    sqlite3 /var/lib/velocity-mcp/data.db ".backup $BACKUP_DIR/database_$DATE.db"
fi

# Backup logs (last 7 days)
tar -czf $BACKUP_DIR/logs_$DATE.tar.gz /var/log/velocity-mcp/ --newer-mtime="7 days ago"

# Cleanup old backups (keep 30 days)
find $BACKUP_DIR -type f -mtime +30 -delete

echo "Backup completed: $DATE"
```

**Cron job:**
```bash
sudo crontab -e

# Add daily backup at 2 AM
0 2 * * * /usr/local/bin/velocity-mcp-backup.sh
```

### Recovery Procedure

**Restore from backup:**
```bash
# Stop service
sudo systemctl stop velocity-mcp

# Restore configuration
sudo tar -xzf /var/backups/velocity-mcp/config_20260830_020000.tar.gz -C /

# Restore database
sudo cp /var/backups/velocity-mcp/database_20260830_020000.db /var/lib/velocity-mcp/data.db

# Set permissions
sudo chown -R velocity-mcp:velocity-mcp /etc/velocity-mcp
sudo chown -R velocity-mcp:velocity-mcp /var/lib/velocity-mcp

# Start service
sudo systemctl start velocity-mcp

# Verify
sudo systemctl status velocity-mcp
curl http://localhost:3000/health
```

---

## Troubleshooting

### Service Won't Start

**Check logs:**
```bash
sudo journalctl -u velocity-mcp -n 50 --no-pager
```

**Common issues:**

1. **Port already in use:**
   ```bash
   sudo lsof -i :3000
   # Kill process or change port in config
   ```

2. **Permission denied:**
   ```bash
   sudo chown -R velocity-mcp:velocity-mcp /var/log/velocity-mcp
   sudo chown -R velocity-mcp:velocity-mcp /var/lib/velocity-mcp
   ```

3. **Invalid configuration:**
   ```bash
   velocity_mcp --config /etc/velocity-mcp/config.toml --check-config
   ```

### High Memory Usage

**Check memory usage:**
```bash
ps aux | grep velocity-mcp
```

**Reduce session timeout:**
```toml
[security]
session_timeout = 900  # 15 minutes
```

### High CPU Usage

**Check CPU usage:**
```bash
top -p $(pgrep velocity-mcp)
```

**Reduce rate limits:**
```toml
[http]
rate_limit = 50
rate_burst = 200
```

### Connection Refused

**Check if service is running:**
```bash
sudo systemctl status velocity-mcp
```

**Check firewall:**
```bash
sudo iptables -L -n | grep 3000
```

**Test locally:**
```bash
curl http://localhost:3000/health
```

### TLS Certificate Errors

**Verify certificate:**
```bash
openssl x509 -in /etc/velocity-mcp/tls/cert.pem -text -noout
```

**Check certificate chain:**
```bash
openssl s_client -connect localhost:3000 -showcerts
```

**Regenerate certificate:**
```bash
sudo openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
  -keyout /etc/velocity-mcp/tls/key.pem \
  -out /etc/velocity-mcp/tls/cert.pem
```

---

## Support

- **Documentation:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/wiki
- **Issues:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/issues
- **Discussions:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/discussions

---

*Last updated: 2026-08-30*
*Version: 3.0.0*
