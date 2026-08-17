# V.E.L.O.C.I.T.Y.-MCP — .qoder Configuration and Knowledge Base

## Summary

Implement the `.qoder` directory structure for the V.E.L.O.C.I.T.Y. NMCP Server workspace. This provides Qoder with project-specific knowledge, skills, and documentation context for the high-performance zero-allocation MCP server, including its dual-protocol architecture (JSON-RPC stdio + NMCP binary shared memory), IPC subsystem, tool registry, and benchmark suite.

---

## Structure Created

```
.qoder/
├── repowiki/
│   ├── en/
│   │   ├── content/
│   │   │   ├── Getting Started.md
│   │   │   ├── Development Guide.md
│   │   │   ├── Troubleshooting & FAQ.md
│   │   │   ├── Core Concepts & Architecture/
│   │   │   │   ├── NMCP Protocol and Dual-Mode Execution.md
│   │   │   │   └── Shared Memory IPC and Zero-Copy Design.md
│   │   │   ├── Protocol Layer/
│   │   │   │   ├── JSON-RPC Stdio Handler.md
│   │   │   │   └── NMCP Binary Zero-Alloc Parser.md
│   │   │   ├── IPC Subsystem/
│   │   │   │   └── Memory-Mapped Ring Buffer.md
│   │   │   ├── Tool Registry and Sandbox/
│   │   │   │   └── NDA Tool Dispatch and C# Delegation.md
│   │   │   └── Performance Benchmarking/
│   │   │       └── Built-in Micro-Benchmark Suite.md
│   │   ├── meta/
│   │   │   └── repowiki-metadata.json
│   │   └── knowledge/
│   │       └── en/
│   │           ├── _index.yaml
│   │           ├── Rust Cargo Single Crate with Release Optimization/
│   │           ├── NMCP Binary Protocol Zero-Alloc Frame Parser/
│   │           ├── Shared Memory IPC with Memory-Mapped Ring Buffer/
│   │           ├── JSON-RPC v2.0 Stdio Protocol Handler/
│   │           ├── NDA Tool Registry and CSharp Core Delegation/
│   │           ├── Built-in Micro-Benchmark Suite/
│   │           └── Proprietary Namibian License Model/
├── skills/
│   ├── rust-code-review/
│   │   └── SKILL.md
│   └── nmcp-protocol-dev/
│       └── SKILL.md
└── specs/
    └── qoder_workspace_setup.md (this file)
```

---

## Key Decisions

1. **Content pages cover all major subsystems**: Architecture, protocol layer, IPC, tool registry, benchmarking
2. **Knowledge cards capture critical constraints**: Release optimization profile, NMCP binary frame format, shared memory layout, JSON-RPC handler, C# delegation, benchmark methodology, license model
3. **Skills adapted for V.E.L.O.C.I.T.Y.-MCP**: Rust code review with project-specific high-risk areas (unsafe blocks, IPC state machine, binary parsing), NMCP protocol development for extending the dual-protocol system
4. **Up-to-date**: All file paths, module boundaries, and architectural details verified against current codebase (August 2026)

---

## Maintenance

When the codebase changes significantly:
1. Update affected content pages in `repowiki/en/content/`
2. Update knowledge cards if module boundaries shift
3. Update `_index.yaml` if new modules are added
4. Update skills if new patterns emerge or conventions change
