#!/usr/bin/env node
/**
 * Ensures `src-tauri/binaries/aria2c-<target-triple>(.exe)` exists for the current
 * host (or `--target <triple>`) so Tauri's `externalBin` bundling always succeeds.
 */
import { execSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const binDir = path.join(root, 'src-tauri', 'binaries');
fs.mkdirSync(binDir, { recursive: true });

function detectTargetTriple() {
  const argIdx = process.argv.indexOf('--target');
  if (argIdx !== -1 && process.argv[argIdx + 1]) {
    return process.argv[argIdx + 1];
  }
  const rustcInfo = execSync('rustc -vV', { encoding: 'utf8' });
  const match = /^host:\s*(\S+)/m.exec(rustcInfo);
  if (!match) {
    throw new Error('Could not determine Rust host target triple from `rustc -vV`');
  }
  return match[1];
}

const triple = detectTargetTriple();
const isWindows = triple.includes('windows');
const outPath = path.join(binDir, `aria2c-${triple}${isWindows ? '.exe' : ''}`);

if (fs.existsSync(outPath)) {
  console.log(`Sidecar already present: ${outPath}`);
  process.exit(0);
}

if (isWindows) {
  const url =
    'https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip';
  console.log(`Downloading aria2c 1.37.0 for ${triple}...`);
  execSync(
    `powershell -NoProfile -Command "` +
      `$z = Join-Path $env:TEMP 'aria2.zip'; ` +
      `$d = Join-Path $env:TEMP 'aria2_extract'; ` +
      `Invoke-WebRequest -Uri '${url}' -OutFile $z -UseBasicParsing; ` +
      `if (Test-Path $d) { Remove-Item -Recurse -Force $d }; ` +
      `Expand-Archive -Path $z -DestinationPath $d -Force; ` +
      `$exe = Get-ChildItem -Recurse -File -Path $d -Filter 'aria2c.exe' | Select-Object -First 1; ` +
      `Copy-Item -Path $exe.FullName -Destination '${outPath}' -Force; ` +
      `Remove-Item -Force $z; Remove-Item -Recurse -Force $d"`,
    { stdio: 'inherit' },
  );
} else {
  const existing = execSync('command -v aria2c || true', { encoding: 'utf8' }).trim();
  if (!existing) {
    throw new Error(
      `aria2c not found in PATH. Install it first (e.g. 'brew install aria2' or 'sudo apt/dnf install aria2'), then re-run.`,
    );
  }
  fs.copyFileSync(existing, outPath);
  fs.chmodSync(outPath, 0o755);
}

console.log(`Prepared sidecar binary: ${outPath}`);
