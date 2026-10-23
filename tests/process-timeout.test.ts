import { spawn, spawnSync } from "node:child_process";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { afterAll, beforeAll, expect, test } from "vitest";

const root = resolve(import.meta.dirname, "..");
const suffix = process.platform === "win32" ? ".exe" : "";
const target = resolve(root, process.env.CARGO_TARGET_DIR || "target");
const cli = join(target, "debug", `tomlsmith-benchmark${suffix}`);
let supportDirectory: string;
let product: string;

beforeAll(async () => {
  const built = spawnSync(
    process.env.CARGO || "cargo",
    ["build", "--locked", "-p", "tomlsmith-benchmark-cli"],
    { cwd: root, encoding: "utf8", timeout: 270_000 },
  );
  expect(built.error).toBeUndefined();
  expect(built.status, built.stderr).toBe(0);
  supportDirectory = await mkdtemp(join(tmpdir(), "tomlsmith-process-test-"));
  product = join(supportDirectory, `product${suffix}`);
  const compiled = spawnSync(
    process.env.RUSTC || "rustc",
    [join(root, "tests", "fixtures", "product.rs"), "--edition=2024", "-o", product],
    { cwd: root, encoding: "utf8", timeout: 30_000 },
  );
  expect(compiled.error).toBeUndefined();
  expect(compiled.status, compiled.stderr).toBe(0);
});

afterAll(async () => {
  if (supportDirectory) await rm(supportDirectory, { recursive: true, force: true });
});

async function readState(directory: string, name: string) {
  return readFile(join(directory, name), "utf8").catch(() => "");
}

async function verify(mode: string, assertion: (result: {
  code: number | null;
  exceededGuard: boolean;
  elapsed: number;
  report: string;
  state: string;
}) => Promise<void>) {
  const state = await mkdtemp(join(tmpdir(), "tomlsmith-process-case-"));
  const environment = Object.fromEntries(
    Object.entries(process.env).filter(([name]) =>
      !name.startsWith("TOMLSMITH_") &&
      !name.startsWith("CARGO_PROFILE_BENCH_") &&
      !["CARGO_INCREMENTAL", "CARGO_BUILD_INCREMENTAL"].includes(name),
    ),
  );
  const fixture = ["hold-stdin", "normal"].includes(mode) ? "v1_0_medium" : "v1_0_small";
  const child = spawn(cli, ["--root", root, "verify", "--json"], {
    cwd: root,
    env: {
      ...environment,
      TOMLSMITH_BIN: product,
      TOMLSMITH_BENCH_PROCESS_TIMEOUT_SECS: "1",
      TOMLSMITH_BENCH_FILTER: `e2e/format/cold-stdin/1.0/${fixture}`,
      TOMLSMITH_TEST_MODE: mode,
      TOMLSMITH_TEST_STATE: state,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let report = "";
  child.stdout.setEncoding("utf8").on("data", (chunk) => { report += chunk; });
  child.stderr.setEncoding("utf8").on("data", (chunk) => { report += chunk; });
  let exceededGuard = false;
  const guard = setTimeout(() => {
    exceededGuard = true;
    child.kill("SIGKILL");
  }, 6_000);
  try {
    const code = await new Promise<number | null>((resolve, reject) => {
      child.on("error", reject);
      child.on("close", resolve);
    });
    const finished = Date.now();
    clearTimeout(guard);
    const productStarted = (await stat(join(state, "parent.pid"))).mtimeMs;
    await assertion({ code, exceededGuard, elapsed: finished - productStarted, report, state });
  } finally {
    clearTimeout(guard);
    child.kill("SIGKILL");
    // The outer guard also cleans products left behind by an unfixed runner.
    for (const role of ["parent", "descendant"]) {
      const pid = Number(await readState(state, `${role}.pid`));
      if (pid > 0) {
        try { process.kill(pid, "SIGKILL"); } catch { /* already reaped */ }
      }
    }
    await rm(state, { recursive: true, force: true });
  }
}

for (const mode of ["hold-stdout", "hold-stderr", "hold-stdin", "live-parent", "late-exit"]) {
  test(`bounds the full operation and terminates descendants: ${mode}`, async () => {
    await verify(mode, async ({ code, exceededGuard, elapsed, report, state }) => {
      expect(exceededGuard, "the benchmark outlived its 1 s process deadline").toBe(false);
      expect(code).toBe(1);
      expect(elapsed).toBeLessThan(mode === "late-exit" ? 1_500 : 2_500);
      expect(report).toContain("timed out after 1000 ms");
      expect(report).toContain("fixture stderr before timeout");
      expect(await readState(state, "descendant.pid")).not.toBe("");
      // Give a killed child time to be reaped, then prove it no longer executes.
      await delay(100);
      const heartbeat = await readState(state, "heartbeat");
      expect(heartbeat).not.toBe("");
      await delay(200);
      expect(await readState(state, "heartbeat")).toBe(heartbeat);
    });
  });
}

test("drains both output pipes while writing large stdin and completes normally", async () => {
  await verify("normal", async ({ code, exceededGuard, report }) => {
    expect(exceededGuard).toBe(false);
    expect(code, report).toBe(0);
    const result = JSON.parse(report);
    expect(result.products.passed).toBe(true);
    expect(result.products.cases).toHaveLength(1);
    expect(result.products.cases[0].idempotent).toBe(true);
  });
});

test("preserves a completed command failure instead of reporting a timeout", async () => {
  await verify("failure", async ({ code, exceededGuard, report }) => {
    expect(exceededGuard).toBe(false);
    expect(code).toBe(1);
    expect(report).toContain("fixture command failed");
    expect(report).toContain("status Some(23)");
    expect(report).not.toContain("timed out");
  });
});

test.runIf(process.platform !== "win32")("reports failed containment without waiting forever for an escaped Unix descendant", async () => {
  await verify("escape-group", async ({ code, exceededGuard, elapsed, report, state }) => {
    expect(exceededGuard).toBe(false);
    expect(code).toBe(1);
    expect(elapsed).toBeLessThan(2_500);
    expect(report).toContain("failed to terminate timed-out");
    expect(await readState(state, "descendant.pid")).not.toBe("");
    // The fixture deliberately moved outside the runner's group. The test's finally block owns
    // its cleanup; the runner must report the containment failure instead of blocking on EOF.
  });
});
