import { spawnSync } from "node:child_process";
import { cp, access } from "node:fs/promises";
import { constants } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const siteRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const wasmRoot = resolve(siteRoot, "wasm");
const wasmManifest = resolve(wasmRoot, "Cargo.toml");
const wasmPackage = resolve(wasmRoot, "pkg");
const distWasmPackage = resolve(siteRoot, "dist/wasm/pkg");
const development = process.argv.includes("--dev");

async function exists(path) {
  try {
    await access(path, constants.F_OK);
    return true;
  } catch {
    return false;
  }
}

if (!await exists(wasmManifest)) {
  throw new Error(`Required browser adapter is missing: ${wasmManifest}`);
}

const wasmPack = spawnSync(
  "wasm-pack",
  [
    "build",
    "wasm",
    "--target",
    "web",
    "--out-dir",
    "pkg",
    "--out-name",
    "ssfilt_web_wasm",
    development ? "--dev" : "--release",
    "--no-opt",
  ],
  { cwd: siteRoot, stdio: "inherit" },
);

if (wasmPack.error) throw wasmPack.error;
if (wasmPack.status !== 0) process.exit(wasmPack.status ?? 1);

const wasmJavaScript = resolve(wasmPackage, "ssfilt_web_wasm.js");
const wasmBinary = resolve(wasmPackage, "ssfilt_web_wasm_bg.wasm");
if (!await exists(wasmJavaScript) || !await exists(wasmBinary)) {
  throw new Error("wasm-pack completed without the browser adapter outputs");
}

const vite = spawnSync("vite", development ? [] : ["build"], {
  cwd: siteRoot,
  stdio: "inherit",
});
if (vite.error) throw vite.error;
if (vite.status !== 0) process.exit(vite.status ?? 1);

if (!development) {
  await cp(wasmPackage, distWasmPackage, { recursive: true });
}
