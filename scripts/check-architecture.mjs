import { readFileSync, readdirSync, statSync } from "node:fs";
import { extname, join, relative, resolve } from "node:path";
import process from "node:process";

const root = resolve(import.meta.dirname, "..");
const failures = [];
const directInvokePattern = /import\s*\{[^}]*\binvoke\b[^}]*\}\s*from\s*["']@tauri-apps\/api\/core["']|__TAURI_INTERNALS__\s*(?:\?\.|\.)invoke\b/;
const sqlPattern = /\b(?:SELECT\s+.+\s+FROM|INSERT\s+INTO|UPDATE\s+[a-z_]+\s+SET|DELETE\s+FROM|CREATE\s+TABLE)\b/i;
const rawStage = "(?:media_check|audio_extract|source_separation|proxy|asr|glossary|translation|script_director|semantic_narration|tts|export)";
const rawStageConstructionPattern = new RegExp(`stage:\\s*["']${rawStage}["'](?:\\.into\\(\\))?`);
const rawStageComparisonPattern = new RegExp(`job\\.stage\\s*==\\s*["']${rawStage}["']`);
const rawStagePersistencePattern = new RegExp(`(?:checkpoint_job|reopen_job|handoff_running_job)\\s*\\([\\s\\S]{0,160}?,\\s*["']${rawStage}["']`);
const frontendStageDispatchPattern = /(?:if|else\s+if|switch)\s*\([^\n]{0,180}\b(?:job|persisted)\.stage\b/;
const legacyProcessingBridgePattern = /desktopBridge\.(?:prepareMedia|runAsr|runTranslation|resumeJob|retryJob|pauseJob|cancelJob|enqueueJob)\s*\(/;

const requiredDocs = [
  "ARCHITECTURE.md",
  "RELIABILITY.md",
  "QUALITY_SCORE.md",
  "docs/architecture/ADR-002-versioned-workflow-runner.md",
  "docs/engineering/WORKFLOW_REFACTOR_EXECUTION_PLAN.md",
];

const legacyLineCaps = new Map([
  ["src-tauri/src/commands.rs", 5300],
  ["src-tauri/src/db.rs", 2850],
  ["src-tauri/src/tts_provider.rs", 1850],
  ["src-tauri/src/media.rs", 1100],
  ["src-tauri/src/exporter.rs", 960],
  ["src-tauri/src/translation.rs", 650],
  ["src-tauri/src/tts.rs", 600],
  ["src-tauri/src/domain.rs", 520],
  ["src/pages/ProvidersPage.tsx", 700],
  ["src/components/EditorPage.tsx", 620],
]);

function text(path) {
  return readFileSync(join(root, path), "utf8");
}

function productionFiles(directory) {
  const absolute = join(root, directory);
  const result = [];
  for (const entry of readdirSync(absolute)) {
    const path = join(absolute, entry);
    const projectPath = relative(root, path);
    if (statSync(path).isDirectory()) {
      if (entry === "tests" || entry === "target" || entry === "gen") continue;
      result.push(...productionFiles(projectPath));
      continue;
    }
    if ([".rs", ".ts", ".tsx"].includes(extname(path)) && !entry.includes(".test.")) {
      result.push(projectPath);
    }
  }
  return result;
}

for (const path of requiredDocs) {
  try {
    if (!text(path).trim()) failures.push(`${path}: required architecture document is empty`);
  } catch {
    failures.push(`${path}: required architecture document is missing`);
  }
}

const sourceFiles = [...productionFiles("src"), ...productionFiles("src-tauri/src")];
for (const path of sourceFiles) {
  const lines = text(path).split(/\r?\n/).length;
  const cap = legacyLineCaps.get(path) ?? 500;
  if (lines > cap) {
    failures.push(`${path}: ${lines} lines exceeds the ${cap}-line architecture cap`);
  }
}

for (const path of sourceFiles.filter((path) => /\.(ts|tsx)$/.test(path) && path !== "src/bridge.ts")) {
  const source = text(path);
  if (directInvokePattern.test(source)) {
    failures.push(`${path}: direct Tauri invoke is only allowed in src/bridge.ts`);
  }
  if (frontendStageDispatchPattern.test(source)) {
    failures.push(`${path}: frontend must not dispatch persisted workflow stages`);
  }
  if (legacyProcessingBridgePattern.test(source)) {
    failures.push(`${path}: use workflow intent methods instead of legacy stage commands`);
  }
}

const tauriHandler = text("src-tauri/src/lib.rs");
for (const command of ["workflow_enqueue", "workflow_start", "workflow_continue", "workflow_retry", "workflow_pause", "workflow_cancel"]) {
  if (!tauriHandler.includes(`workflow_commands::${command}`)) {
    failures.push(`src-tauri/src/lib.rs: missing workflow intent command ${command}`);
  }
}
for (const command of ["media_prepare", "asr_run", "translation_run", "job_start", "job_resume", "job_retry", "job_checkpoint"]) {
  if (tauriHandler.includes(`commands::${command}`)) {
    failures.push(`src-tauri/src/lib.rs: legacy orchestration command remains exposed: ${command}`);
  }
}

for (const path of sourceFiles.filter((path) => path.endsWith(".rs")
  && path !== "src-tauri/src/db.rs"
  && !path.startsWith("src-tauri/src/infrastructure/"))) {
  if (sqlPattern.test(text(path))) {
    failures.push(`${path}: SQL is only allowed in the SQLite infrastructure module`);
  }
}

const rustDomain = text("src-tauri/src/domain.rs");
const tsDomain = text("src/domain.ts");
if (!/pub struct JobSummary[\s\S]*?pub stage: JobStage,/.test(rustDomain)) {
  failures.push("src-tauri/src/domain.rs: JobSummary.stage must use JobStage");
}
if (!/export interface PersistedJob[\s\S]*?stage: JobStage;/.test(tsDomain)) {
  failures.push("src/domain.ts: PersistedJob.stage must use JobStage");
}

const rustSources = sourceFiles.filter((path) => path.endsWith(".rs") && path !== "src-tauri/src/domain.rs")
  .map((path) => `${path}\n${text(path)}`)
  .join("\n");
if (rawStageConstructionPattern.test(rustSources)) {
  failures.push("Rust job construction contains a raw stage string; use JobStage");
}
if (rawStageComparisonPattern.test(rustSources)) {
  failures.push("Rust job stage comparison contains a raw string; use JobStage");
}
if (rawStagePersistencePattern.test(rustSources)) {
  failures.push("Rust workflow persistence call contains a raw stage string; use JobStage");
}

const guardFixtures = [
  ["direct Tauri invoke", directInvokePattern, 'import { invoke } from "@tauri-apps/api/core";'],
  ["SQL outside infrastructure", sqlPattern, 'let query = "INSERT INTO jobs (id) VALUES (?)";'],
  ["raw stage construction", rawStageConstructionPattern, 'JobSummary { stage: "asr".into() }'],
  ["raw stage comparison", rawStageComparisonPattern, 'if job.stage == "export" {}'],
  ["raw stage persistence", rawStagePersistencePattern, 'database.checkpoint_job(&id, "tts", 63, "started")'],
  ["frontend workflow stage dispatch", frontendStageDispatchPattern, 'if (persisted.stage === "asr") runAsr();'],
  ["legacy processing bridge", legacyProcessingBridgePattern, 'desktopBridge.prepareMedia(projectId, jobId);'],
];
for (const [name, pattern, fixture] of guardFixtures) {
  pattern.lastIndex = 0;
  if (!pattern.test(fixture)) failures.push(`architecture guard self-test did not reject ${name}`);
}

if (failures.length) {
  console.error("Architecture checks failed:\n");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

console.log(`Architecture checks passed (${sourceFiles.length} production source files).`);
