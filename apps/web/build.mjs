import { copyFile, mkdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");
const cargo = spawnSync(
  "cargo",
  ["build", "-p", "fastchart-wasm", "--target", "wasm32-unknown-unknown", "--release"],
  { cwd: root, stdio: "inherit", shell: process.platform === "win32" },
);
if (cargo.status !== 0) process.exit(cargo.status ?? 1);

const assets = ["index.html", "styles.css", "app.js", "worker.js", "zip.js"];
await mkdir(resolve(here, "pkg"), { recursive: true });
await mkdir(resolve(here, "dist/pkg"), { recursive: true });
await copyFile(
  resolve(root, "target/wasm32-unknown-unknown/release/fastchart_wasm.wasm"),
  resolve(here, "pkg/fastchart.wasm"),
);
for (const asset of assets) await copyFile(resolve(here, asset), resolve(here, "dist", asset));
await copyFile(resolve(here, "pkg/fastchart.wasm"), resolve(here, "dist/pkg/fastchart.wasm"));
console.log("FastChart web build ready in apps/web/dist");
