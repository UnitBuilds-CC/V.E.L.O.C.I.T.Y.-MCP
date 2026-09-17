# VELOCITY-MCP Edge Operations Runbook

Operational procedures for running VELOCITY-MCP on Wasmer Edge in production.

---

## Table of Contents

1. [Monitoring Setup](#1-monitoring-setup)
2. [Alerting Thresholds](#2-alerting-thresholds)
3. [Scaling Guidelines](#3-scaling-guidelines)
4. [Backup and Recovery](#4-backup-and-recovery)
5. [Incident Response Playbook](#5-incident-response-playbook)
6. [Log Analysis Patterns](#6-log-analysis-patterns)
7. [Routine Maintenance](#7-routine-maintenance)
8. [Cost Management](#8-cost-management)

---

## 1. Monitoring Setup

### Key Metrics to Watch

| Metric | Source | Healthy Range | Warning | Critical |
|--------|--------|---------------|---------|----------|
| Health check status | `GET /health` | 200 OK | Intermittent 503 | Persistent 503 |
| Request latency (p50) | `wasmer edge metrics` | <20ms | >50ms | >200ms |
| Request latency (p99) | `wasmer edge metrics` | <100ms | >500ms | >2000ms |
| Error rate | `wasmer edge metrics` | <0.1% | >1% | >5% |
| Active instances | `wasmer edge list` | 1-3 | 0 (all scaled down) | max_instances reached |
| Cold start frequency | Logs | <10% of requests | >30% of requests | >50% of requests |
| Instruction limit hits | Logs | 0 | >0.1% of requests | >1% of requests |
| Memory utilization | `wasmer edge metrics` | <80% of limit | >90% | OOM errors |

### Monitoring Commands

```bash
# List all deployments and their status
wasmer edge list

# View real-time logs with filtering
wasmer edge logs <app-name> --follow

# View metrics dashboard
wasmer edge metrics <app-name>

# Check specific instance health
curl -s https://<your-app>.wasmer.app/health | python -m json.tool
```

### Automated Health Monitoring Script

Save this as `monitor-edge.sh` and run via cron every 5 minutes:

```bash
#!/bin/bash
ENDPOINT="https://<your-app>.wasmer.app"
LOGFILE="/var/log/velocity-edge-monitor.log"
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# Health check
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$ENDPOINT/health" --max-time 10)
LATENCY=$(curl -s -o /dev/null -w "%{time_total}" "$ENDPOINT/health" --max-time 10)

echo "$TIMESTAMP health_status=$HTTP_CODE latency=${LATENCY}s" >> "$LOGFILE"

if [ "$HTTP_CODE" != "200" ]; then
    echo "$TIMESTAMP ALERT: Health check returned $HTTP_CODE" >> "$LOGFILE"
    # Trigger alert (PagerDuty, Slack, email, etc.)
fi

# MCP endpoint check
MCP_RESPONSE=$(curl -s -X POST "$ENDPOINT/mcp" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"ping","id":1}' \
    --max-time 10)

if ! echo "$MCP_RESPONSE" | grep -q '"result"'; then
    echo "$TIMESTAMP ALERT: MCP ping failed - $MCP_RESPONSE" >> "$LOGFILE"
fi
```

### Prometheus Integration

If you run a Prometheus instance, add this scrape target to monitor the Edge endpoint externally:

```yaml
scrape_configs:
  - job_name: 'velocity-mcp-edge'
    metrics_path: /health
    static_configs:
      - targets: ['<your-app>.wasmer.app']
    scrape_interval: 30s
```

Note: Wasmer Edge does not expose a full Prometheus `/metrics` endpoint like the native deployment. Use the Wasmer CLI and external health checks for monitoring.

---

## 2. Alerting Thresholds

### When to Page (Immediate Action Required)

| Condition | Threshold | Action |
|-----------|-----------|--------|
| Health check down | 3 consecutive failures | Page on-call |
| Error rate spike | >5% over 5 minutes | Page on-call |
| Full outage | No healthy instances for 5 min | Page on-call + escalate |
| OOM errors | Any occurrence | Page on-call |

### When to Notify (Next Business Day)

| Condition | Threshold | Action |
|-----------|-----------|--------|
| Elevated latency | p99 > 500ms for 15 min | Slack notification |
| Cold start frequency | >30% of requests | Slack notification |
| Instruction limit hits | >0.5% of requests | Slack notification |
| Approaching free tier limit | >80% of monthly quota used | Email notification |

### When to Log Only (Review in Weekly Ops Meeting)

| Condition | Threshold | Action |
|-----------|-----------|--------|
| Occasional errors | <0.1% error rate | Log for trend analysis |
| Slow tool executions | p50 > 20ms | Log for optimization review |
| Scale-up events | Any auto-scaling event | Log for capacity planning |

### Alert Configuration Example (PagerDuty Webhook)

```bash
#!/bin/bash
# alert-pagerduty.sh - Send alert to PagerDuty

ROUTING_KEY="your-pagerduty-routing-key"
SEVERITY="$1"     # "critical", "warning", "info"
SUMMARY="$2"      # Human-readable summary
SOURCE="velocity-mcp-edge"

curl -X POST https://events.pagerduty.com/v2/enqueue \
  -H "Content-Type: application/json" \
  -d "{
    \"routing_key\": \"$ROUTING_KEY\",
    \"event_action\": \"trigger\",
    \"payload\": {
      \"summary\": \"$SUMMARY\",
      \"severity\": \"$SEVERITY\",
      \"source\": \"$SOURCE\",
      \"component\": \"wasmer-edge\",
      \"custom_details\": {
        \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",
        \"environment\": \"production\"
      }
    }
  }"
```

---

## 3. Scaling Guidelines

### Free Tier Limits

| Resource | Free Tier Limit | Recommended Setting |
|----------|----------------|-------------------|
| Memory per instance | 128MB | 128MB |
| Instructions per request | 5M | 5,000,000 |
| Max instances | 3 | 3 |
| Monthly invocations | ~1M | Monitor usage |
| Request body size | 1MB | 1,048,576 bytes |
| Request timeout | 30s | 30 seconds |

### When to Scale Up (Increase Limits)

**Increase `max_instances`** when:
- Requests queue for more than 2 seconds
- Auto-scaling events are frequent
- Users report intermittent timeouts

**Increase `memory_mb`** when:
- OOM errors appear in logs
- Tool executions fail with memory errors
- WASM plugin loading exceeds available memory

**Increase `instruction_limit`** when:
- Complex tools are terminated prematurely
- Users report "instruction limit exceeded" errors
- Data transformation tools fail on large inputs

### When to Scale Down (Reduce Costs)

**Decrease `max_instances`** when:
- Instances are consistently underutilized
- Traffic is predictable and low-volume
- Costs exceed budget

**Set `min_instances = 0`** when:
- Cost is the primary concern
- Occasional cold starts (100-500ms) are acceptable
- Traffic is sporadic

### Scaling Decision Matrix

| Daily Requests | min_instances | max_instances | memory_mb | Expected Cost |
|---------------|---------------|---------------|-----------|---------------|
| <1,000 | 0 | 1 | 128 | Free tier |
| 1,000-10,000 | 0 | 3 | 128 | Free tier |
| 10,000-100,000 | 1 | 10 | 256 | ~$5-20/month |
| 100,000+ | 2 | 50 | 512 | ~$50-200/month |

---

## 4. Backup and Recovery

### What to Back Up

Wasmer Edge is serverless, so there is no persistent state to back up in the traditional sense. However, you should maintain backups of:

| Asset | Location | Backup Method |
|-------|----------|---------------|
| WASM binary | Git releases | Tag releases in Git |
| Configuration (`wasmer.toml`) | Git repository | Version controlled |
| Plugin manifests | Git repository | Version controlled |
| API keys | Secrets manager | Wasmer secrets / env vars |
| Deployment logs | Wasmer dashboard | Export periodically |

### Recovery Procedures

**Full Redeploy from Scratch:**

```bash
# 1. Clone the repository at the tagged version
git clone --branch v3.2.0 https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP.git
cd V.E.L.O.C.I.T.Y.-MCP

# 2. Build
python deploy/build-edge.py

# 3. Verify binary
ls -lh target/wasm32-wasmer-wasi/release/velocity-edge.wasm

# 4. Deploy
wasmer deploy

# 5. Verify health
curl https://<your-app>.wasmer.app/health
```

**Rollback to Previous Version:**

```bash
# 1. Checkout previous version
git checkout v3.1.0  # or the previous known-good tag

# 2. Rebuild
python deploy/build-edge.py

# 3. Redeploy (overwrites current deployment)
wasmer deploy

# 4. Verify
curl https://<your-app>.wasmer.app/health
```

**Disaster Recovery:**

If the Wasmer Edge platform itself has an outage:
1. Check [wasmer.io/status](https://wasmer.io/status) for platform status
2. Fall back to native binary deployment using Docker or bare metal
3. Update DNS or client configuration to point to fallback server
4. Restore Edge deployment when platform recovers

### Failover Architecture

For production use, consider a two-tier failover setup:

```
Primary:   Wasmer Edge (serverless, global)
Fallback:  Native binary on a VPS (single region)
```

Configure your DNS with a failover record:
- Primary: Edge endpoint (health-checked)
- Secondary: VPS endpoint (activated when primary fails)

---

## 5. Incident Response Playbook

### Incident Severity Levels

| Level | Definition | Response Time | Example |
|-------|-----------|---------------|---------|
| **SEV-1** | Complete outage, no requests served | Immediate | All instances down |
| **SEV-2** | Major degradation, >50% errors | 15 minutes | OOM on all instances |
| **SEV-3** | Partial degradation, elevated latency | 1 hour | Cold start storms |
| **SEV-4** | Minor issue, no user impact | Next business day | Logging errors |

### SEV-1: Complete Outage

**Symptoms:** Health check returns 503 or times out for all instances.

**Immediate Actions:**

```bash
# Step 1: Confirm the outage
curl -v https://<your-app>.wasmer.app/health

# Step 2: Check Wasmer platform status
# Visit: https://wasmer.io/status or check Wasmer's status page

# Step 3: Check deployment status
wasmer edge list

# Step 4: View recent logs for errors
wasmer edge logs <app-name> --tail 50

# Step 5: If platform is healthy but deployment is broken, redeploy
./deploy-edge.sh
```

**If redeployment does not fix it:**

```bash
# Step 6: Try rolling back to previous version
git checkout <previous-known-good-tag>
python deploy/build-edge.py
wasmer deploy

# Step 7: If still failing, activate failover
# Point DNS/client config to native binary fallback
```

**Post-Incident:**
1. Write incident report within 24 hours
2. Identify root cause
3. Create follow-up tasks to prevent recurrence

### SEV-2: Major Degradation

**Symptoms:** Error rate >5%, many requests failing.

**Immediate Actions:**

```bash
# Step 1: Check logs for specific error patterns
wasmer edge logs <app-name> --tail 100 | grep -i "error\|oom\|instruction"

# Step 2: If OOM errors, increase memory and redeploy
# Edit wasmer.toml: memory_mb = 256
python deploy/build-edge.py
wasmer deploy

# Step 3: If instruction limit errors, increase limit and redeploy
# Edit wasmer.toml: instruction_limit = 10_000_000
python deploy/build-edge.py
wasmer deploy
```

### SEV-3: Elevated Latency

**Symptoms:** p99 latency >500ms, users reporting slowness.

**Actions:**

```bash
# Step 1: Check if cold starts are the cause
wasmer edge logs <app-name> --tail 50 | grep -i "cold\|start\|init"

# Step 2: If cold starts, increase min_instances to 1
# Edit wasmer.toml: min_instances = 1
wasmer deploy

# Step 3: Check if specific tools are slow
# Review tool execution logs and optimize hot paths
```

### SEV-4: Minor Issue

**Actions:**

```bash
# Step 1: Review logs at leisure
wasmer edge logs <app-name> --tail 200

# Step 2: Check metrics for trends
wasmer edge metrics <app-name>

# Step 3: Create ticket for investigation
```

---

## 6. Log Analysis Patterns

### Log Format

VELOCITY-MCP Edge outputs structured JSON logs via `tracing-subscriber`:

```json
{"timestamp":"2026-09-12T14:30:00Z","level":"INFO","target":"velocity_edge","fields":{"method":"POST","path":"/mcp","message":"Received HTTP request"}}
```

### Useful Log Patterns

**Find all errors:**
```bash
wasmer edge logs <app-name> --tail 500 | grep '"level":"ERROR"'
```

**Find instruction limit violations:**
```bash
wasmer edge logs <app-name> --tail 500 | grep -i "instruction.*limit\|metering.*exceeded"
```

**Find out-of-memory errors:**
```bash
wasmer edge logs <app-name> --tail 500 | grep -i "out of memory\|OOM\|memory.*exceeded"
```

**Find slow requests (high-latency tools):**
```bash
wasmer edge logs <app-name> --tail 500 | grep -i "timeout\|slow\|deadline"
```

**Track specific tool executions:**
```bash
wasmer edge logs <app-name> --tail 500 | grep "tools/call" | grep "tool_name"
```

**Count errors per time window:**
```bash
wasmer edge logs <app-name> --tail 1000 | grep '"level":"ERROR"' | \
  cut -d'"' -f4 | sort | uniq -c | sort -rn
```

**Monitor request volume:**
```bash
wasmer edge logs <app-name> --tail 1000 | grep "Received HTTP request" | wc -l
```

### Log Level Guidelines

| Level | When to Use | Cost Impact |
|-------|------------|-------------|
| `error` | Only failures | Minimal overhead |
| `warn` | Free tier default | Low overhead |
| `info` | Debugging, production monitoring | Moderate overhead |
| `debug` | Active troubleshooting | High overhead, temporary use only |
| `trace` | Deep diagnostics | Very high overhead, never leave on |

Change log level dynamically via environment variable:

```toml
[edge.env]
RUST_LOG = "info"  # Temporarily increase for debugging
```

---

## 7. Routine Maintenance

### Weekly Tasks

- [ ] Review error rate trends (should be <0.1%)
- [ ] Check monthly invocation count against free tier limit
- [ ] Review log output for new error patterns
- [ ] Verify health check is consistently returning 200

### Monthly Tasks

- [ ] Update Rust toolchain and rebuild WASM binary
- [ ] Review and update dependencies (`cargo update`)
- [ ] Audit API key rotation (if applicable)
- [ ] Review scaling configuration against actual traffic
- [ ] Check Wasmer Edge changelog for platform updates
- [ ] Review and archive old logs

### Quarterly Tasks

- [ ] Full disaster recovery drill (deploy from scratch)
- [ ] Review and test failover procedure
- [ ] Benchmark performance against baseline
- [ ] Review security configuration
- [ ] Update documentation

### Update Procedure

```bash
# 1. Pull latest changes
git pull origin main

# 2. Update dependencies
cargo update

# 3. Run tests locally
cargo test --all-features

# 4. Build WASM binary
python deploy/build-edge.py

# 5. Deploy
wasmer deploy

# 6. Verify
curl https://<your-app>.wasmer.app/health
```

---

## 8. Cost Management

### Free Tier Budget Tracking

The free tier provides approximately 1 million invocations per month. Track usage:

```bash
# Check current month's invocation count
wasmer edge metrics <app-name>
```

### Cost Optimization Checklist

- [ ] `min_instances = 0` to eliminate idle charges
- [ ] `RUST_LOG = "warn"` to minimize logging overhead
- [ ] `memory_mb = 128` (minimum for your workload)
- [ ] `instruction_limit = 5_000_000` (minimum for your tools)
- [ ] `max_instances = 3` (minimum for your traffic)
- [ ] Monitor request volume weekly
- [ ] Set up alert at 80% of free tier quota

### Cost Estimation

| Scenario | Requests/Day | Instances | Estimated Monthly Cost |
|----------|-------------|-----------|----------------------|
| Development/testing | 50 | 0-1 | $0 (free tier) |
| Small team | 500 | 0-3 | $0 (free tier) |
| Growing product | 5,000 | 1-5 | $5-15 |
| Production load | 50,000 | 2-20 | $50-150 |
| High traffic | 500,000+ | 5-50 | $300-1000 |

---

## Quick Reference Card

| Task | Command |
|------|---------|
| Check health | `curl https://<app>.wasmer.app/health` |
| View logs | `wasmer edge logs <app-name>` |
| View metrics | `wasmer edge metrics <app-name>` |
| List deployments | `wasmer edge list` |
| Redeploy | `./deploy-edge.sh` |
| Rollback | `git checkout <tag> && python deploy/build-edge.py && wasmer deploy` |
| Increase memory | Edit `wasmer.toml`, set `memory_mb`, redeploy |
| Increase instances | Edit `wasmer.toml`, set `max_instances`, redeploy |
| Check platform status | Visit `wasmer.io/status` |
