import {
  Callout,
  Divider,
  H1,
  MetricsGrid,
  Progress,
  ReportSection,
  ReportShell,
  Stack,
  Table,
  Text,
  Timeline,
} from "qoder/canvas";

const headlineMetrics = [
  { label: "Tests Passing", value: "34", tone: "success" as const, description: "0 failures" },
  { label: "Issues Fixed", value: "8", description: "Deep audit findings" },
  { label: "Files Modified", value: "5", description: "All core modules" },
  { label: "Build Status", value: "Clean", tone: "success" as const, description: "Debug + Release" },
];

const timelineEvents = [
  {
    id: "v1",
    timestamp: "Phase 1",
    title: "Centralized Version + Dead Code Removal",
    description: "Added VERSION constant to main.rs, removed unused Arc<AtomicBool> shutdown_flag",
    state: "completed" as const,
    tone: "success" as const,
  },
  {
    id: "v2",
    timestamp: "Phase 2",
    title: "Stdio Non-Blocking Shutdown",
    description: "Replaced blocking read_line with reader thread + mpsc channel + recv_timeout(200ms)",
    state: "completed" as const,
    tone: "success" as const,
  },
  {
    id: "v3",
    timestamp: "Phase 3",
    title: "Max Request Size Limit",
    description: "Added 1 MB MAX_REQUEST_SIZE check before JSON parsing in stdio mode",
    state: "completed" as const,
    tone: "success" as const,
  },
  {
    id: "v4",
    timestamp: "Phase 4",
    title: "Kill Child on Timeout",
    description: "Replaced thread-based wait with try_wait() polling loop + child.kill() on timeout",
    state: "completed" as const,
    tone: "success" as const,
  },
  {
    id: "v5",
    timestamp: "Phase 5",
    title: "Shmem Fence Synchronization",
    description: "Added SeqCst fences between length writes and state transitions, documented safety invariants",
    state: "completed" as const,
    tone: "success" as const,
  },
  {
    id: "v6",
    timestamp: "Phase 6",
    title: "Buffer Cleanup + Safety Docs",
    description: "Shmem buffer file removed on graceful shutdown, safety comments on all unsafe blocks",
    state: "completed" as const,
    tone: "success" as const,
  },
];

const changedFiles: string[][] = [
  ["src/main.rs", "128 LOC", "VERSION constant, removed dead Arc, version in help text"],
  ["src/protocol/json_rpc.rs", "293 LOC", "Reader thread, recv_timeout, MAX_REQUEST_SIZE, version ref"],
  ["src/protocol/nmcp_binary.rs", "275 LOC", "sync_fence() calls, buffer cleanup, version ref"],
  ["src/registry.rs", "339 LOC", "try_wait() polling, child.kill() on timeout"],
  ["src/ipc/shmem.rs", "325 LOC", "SeqCst fences, sync_fence(), safety docs, write ordering fix"],
];

const readinessItems = [
  { label: "Shmem Length Synchronization", value: 100, tone: "success" as const },
  { label: "Child Process Cleanup", value: 100, tone: "success" as const },
  { label: "Non-Blocking Shutdown", value: 100, tone: "success" as const },
  { label: "Dead Code Eliminated", value: 100, tone: "success" as const },
  { label: "Buffer File Cleanup", value: 100, tone: "success" as const },
  { label: "Safety Documentation", value: 100, tone: "success" as const },
  { label: "Request Size Limit", value: 100, tone: "success" as const },
  { label: "Version Centralization", value: 100, tone: "success" as const },
];

export default function PhaseTwoReport() {
  return (
    <ReportShell
      width="wide"
      ariaLabel="V.E.L.O.C.I.T.Y.-MCP Phase Two Hardening Report"
    >
      <Stack gap="section">
        <header>
          <Stack gap="component">
            <H1>Phase Two: Deep Production Hardening</H1>
            <Text tone="secondary">
              8 remaining issues from deep code audit resolved. All unsafe blocks
              documented, IPC synchronization hardened, shutdown made functional.
            </Text>
            <MetricsGrid variant="header" columns={4} items={headlineMetrics} />
          </Stack>
        </header>

        <Divider />

        <ReportSection
          title="Accomplishment Summary"
          description="All 8 deep-audit findings resolved across 5 source files."
          divided
        >
          <Stack gap="component">
            <Callout tone="success" title="Deep Hardening Complete">
              IPC length fields now synchronized via SeqCst fences, child processes
              are killed on timeout, stdio shutdown is non-blocking via reader thread,
              shmem buffer files are cleaned up on exit, and all unsafe blocks have
              safety documentation.
            </Callout>
            <Stack gap="small">
              {readinessItems.map((item) => (
                <Stack
                  key={item.label}
                  gap="small"
                  align="center"
                  style={{ flexDirection: "row" }}
                >
                  <Text size="small" style={{ minWidth: 220 }}>
                    {item.label}
                  </Text>
                  <Progress
                    value={item.value}
                    max={100}
                    tone={item.tone}
                    format="percent"
                    style={{ flex: 1 }}
                  />
                </Stack>
              ))}
            </Stack>
          </Stack>
        </ReportSection>

        <ReportSection
          title="Execution Timeline"
          description="Key steps in order of implementation"
          divided
        >
          <Timeline events={timelineEvents} density="compact" />
        </ReportSection>

        <ReportSection
          title="Changed Files"
          description="All source modules modified with deep hardening"
          divided
        >
          <Table
            headers={["File", "Size", "Changes"]}
            rows={changedFiles}
            density="compact"
          />
        </ReportSection>

        <ReportSection
          title="Issue Detail"
          description="Each finding with root cause and fix applied"
          divided
        >
          <Table
            headers={["#", "Issue", "Root Cause", "Fix"]}
            rows={[
              [
                "1",
                "Length fields not atomic",
                "Only state byte used AtomicU8; length fields were plain bytes with no ordering guarantee",
                "Added SeqCst fences between length writes and state transitions; documented x86_64 alignment rationale",
              ],
              [
                "2",
                "Timeout doesn't kill child",
                "wait_with_output moved child into thread; no way to kill on timeout",
                "Replaced with try_wait() polling loop; child.kill() + child.wait() on timeout",
              ],
              [
                "3",
                "Stdio shutdown non-functional",
                "read_line() blocks indefinitely; shutdown flag never polled between reads",
                "Reader thread + mpsc channel + recv_timeout(200ms) for periodic shutdown checks",
              ],
              [
                "4",
                "Dead shutdown_flag Arc",
                "Local Arc<AtomicBool> created but never read; only static SHUTDOWN used",
                "Removed dead code and unused Arc import",
              ],
              [
                "5",
                "No buffer file cleanup",
                "Shmem buffer file persisted on disk with stale state after shutdown",
                "drop(buffer) + fs::remove_file(buffer_path) on graceful shutdown",
              ],
              [
                "6",
                "Missing safety comments",
                "unsafe blocks in shmem and binary parser lacked Safety documentation",
                "Added # Safety sections explaining pointer cast safety invariants",
              ],
              [
                "7",
                "No request size limit",
                "Stdio mode read unbounded lines before JSON parsing",
                "Added MAX_REQUEST_SIZE (1 MB) check before serde_json::from_str",
              ],
              [
                "8",
                "Version hardcoded in 5 places",
                "\"1.0.0\" duplicated across json_rpc.rs, nmcp_binary.rs, main.rs",
                "Centralized as pub const VERSION in main.rs; all handlers reference crate::VERSION",
              ],
            ]}
            density="compact"
          />
        </ReportSection>

        <ReportSection title="Verification Evidence" divided>
          <Stack gap="component">
            <Table
              headers={["Check", "Result", "Details"]}
              rows={[
                ["cargo check", "Clean", "Zero warnings after cargo clean"],
                ["cargo test", "34 passed, 0 failed", "All unit tests green"],
                ["cargo build --release", "Clean compile", "LTO, opt-level 3, panic=abort, strip"],
                [
                  "Fence ordering",
                  "Verified",
                  "SeqCst fences between write_output and set_state in all paths",
                ],
                [
                  "Timeout kill",
                  "Verified",
                  "try_wait() loop with child.kill() + child.wait() on expiry",
                ],
                [
                  "Stdio shutdown",
                  "Verified",
                  "Reader thread + recv_timeout(200ms) polls shutdown flag",
                ],
                [
                  "Buffer cleanup",
                  "Verified",
                  "drop(buffer) + remove_file on loop exit",
                ],
                [
                  "Request limit",
                  "Verified",
                  "1 MB MAX_REQUEST_SIZE enforced before parsing",
                ],
              ]}
              density="compact"
            />
          </Stack>
        </ReportSection>

        <ReportSection title="Final Outcome" divided>
          <Callout tone="success" title="Fully Production Hardened">
            Combined with Phase 1 (atomic state, C# timeout, configurable paths,
            test suite, graceful shutdown, structured logging, health check, path
            validation), the V.E.L.O.C.I.T.Y.-MCP server now has comprehensive
            production hardening across all identified dimensions. 34 tests pass,
            clean release build, all unsafe blocks documented.
          </Callout>
          <Stack gap="small">
            <Text size="small" tone="secondary">
              Phase 1 (10 items): Atomic state, C# timeout, configurable paths,
              34-test suite, graceful shutdown, structured logging, initialize in
              shmem, path validation, health check, dependencies.
            </Text>
            <Text size="small" tone="secondary">
              Phase 2 (8 items): Fence synchronization, child kill on timeout,
              non-blocking shutdown, dead code removal, buffer cleanup, safety
              docs, request size limit, version constant.
            </Text>
            <Text size="small" tone="secondary" weight="semibold">
              Total: 18 production hardening items completed. 0 remaining.
            </Text>
          </Stack>
        </ReportSection>
      </Stack>
    </ReportShell>
  );
}
