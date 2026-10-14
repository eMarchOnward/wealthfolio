import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";

const forwardedViteArgs = process.argv.slice(2);
const children = new Set();
let stopping = false;

function spawnPnpm(args) {
  const child = spawn("pnpm", args, { stdio: "inherit" });
  children.add(child);
  child.once("exit", () => children.delete(child));
  return child;
}

// On Windows, killing the pnpm wrapper leaves the vite process it started
// running (holding port 1420 and the watcher), so end the whole process tree.
function killTree(child) {
  if (child.exitCode !== null || child.pid === undefined) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    child.kill("SIGTERM");
  }
}

async function stop(code) {
  if (stopping) return;
  stopping = true;
  const exits = Array.from(children, (child) => once(child, "exit").catch(() => undefined));
  for (const child of children) {
    killTree(child);
  }
  await Promise.all(exits);
  process.exitCode = code;
}

process.once("SIGINT", () => void stop(130));
process.once("SIGTERM", () => void stop(143));
// Last resort when this script exits without running stop() (e.g. its parent ends it).
process.once("exit", () => {
  for (const child of children) killTree(child);
});

const initialBuild = spawnPnpm([
  "exec",
  "vite",
  "build",
  "--config",
  "vite.addon-sandbox.config.ts",
]);
const [initialCode] = await once(initialBuild, "exit");
if (initialCode !== 0) {
  process.exitCode = typeof initialCode === "number" ? initialCode : 1;
} else {
  const runtimeWatcher = spawnPnpm([
    "exec",
    "vite",
    "build",
    "--config",
    "vite.addon-sandbox.config.ts",
    "--watch",
  ]);
  const vite = spawnPnpm(["exec", "vite", ...forwardedViteArgs]);

  runtimeWatcher.once("exit", (code) => void stop(typeof code === "number" ? code : 1));
  vite.once("exit", (code) => void stop(typeof code === "number" ? code : 1));
}
