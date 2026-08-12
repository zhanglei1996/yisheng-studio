#!/usr/bin/env node
import { spawn } from "node:child_process";

const commands = process.argv.slice(2);
if (!commands.length) {
  console.error("Usage: node scripts/run-parallel.mjs \"command one\" \"command two\"");
  process.exit(2);
}

const startedAt = performance.now();
let stopping = false;
const children = new Map();

const stopOthers = (except) => {
  if (stopping) return;
  stopping = true;
  for (const [command, child] of children) {
    if (command !== except && child.exitCode === null) child.kill("SIGTERM");
  }
};

const results = await Promise.all(commands.map((command) => new Promise((resolve) => {
  const commandStartedAt = performance.now();
  console.log(`\n▶ ${command}`);
  const child = spawn(command, {
    cwd: process.cwd(),
    env: process.env,
    shell: true,
    stdio: "inherit",
  });
  children.set(command, child);
  child.on("error", (error) => {
    console.error(`✗ ${command}: ${error.message}`);
    stopOthers(command);
    resolve({ command, code: 1, durationMs: performance.now() - commandStartedAt });
  });
  child.on("exit", (code, signal) => {
    const durationMs = performance.now() - commandStartedAt;
    if ((code ?? 1) !== 0) stopOthers(command);
    resolve({ command, code: code ?? (signal ? 1 : 0), durationMs });
  });
})));

for (const result of results) {
  const marker = result.code === 0 ? "✓" : "✗";
  console.log(`${marker} ${result.command} (${(result.durationMs / 1000).toFixed(1)}s)`);
}
console.log(`总耗时 ${(performance.now() - startedAt) / 1000 < 0.1 ? "<0.1" : ((performance.now() - startedAt) / 1000).toFixed(1)}s`);
process.exit(results.some((result) => result.code !== 0) ? 1 : 0);
