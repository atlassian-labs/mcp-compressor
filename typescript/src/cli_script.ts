import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

const SCRIPT_MARKER = 'mcp-compressor ts cli-mode script';

export async function findScriptDir(): Promise<{ dir: string; onPath: boolean }> {
  const pathDirs = new Set((process.env.PATH ?? '').split(path.delimiter).filter(Boolean).map((entry) => path.resolve(entry)));
  const candidates = process.platform === 'win32'
    ? [path.join(process.env.APPDATA ?? os.homedir(), 'npm')]
    : [path.join(os.homedir(), '.local', 'bin'), path.join(os.homedir(), 'bin')];

  for (const candidate of candidates) {
    const resolved = path.resolve(candidate);
    try {
      const stats = await fs.stat(resolved);
      if (stats.isDirectory() && pathDirs.has(resolved)) {
        return { dir: resolved, onPath: true };
      }
    } catch {
      // ignore missing dirs
    }
  }

  return { dir: process.cwd(), onPath: false };
}

export async function generateCliScript(
  cliName: string,
  bridgeUrl: string,
  scriptDir?: string,
): Promise<{ scriptPath: string; onPath: boolean }> {
  const location = scriptDir ? { dir: scriptDir, onPath: true } : await findScriptDir();
  await fs.mkdir(location.dir, { recursive: true });
  const scriptPath = path.join(location.dir, process.platform === 'win32' ? `${cliName}.cmd` : cliName);
  const content = process.platform === 'win32' ? windowsScript(bridgeUrl) : unixScript(bridgeUrl);
  await fs.writeFile(scriptPath, content, { encoding: 'utf8', mode: 0o755 });
  if (process.platform !== 'win32') {
    await fs.chmod(scriptPath, 0o755);
  }
  return { scriptPath, onPath: location.onPath };
}

export async function removeCliScript(scriptPath: string): Promise<void> {
  try {
    const content = await fs.readFile(scriptPath, 'utf8');
    if (!content.includes(SCRIPT_MARKER)) {
      return;
    }
    await fs.unlink(scriptPath);
  } catch {
    // ignore cleanup failures
  }
}

function unixScript(bridgeUrl: string): string {
  return `#!/usr/bin/env bash
# ${SCRIPT_MARKER}
set -euo pipefail

BRIDGE_URL=${shellQuote(bridgeUrl)}

if [ "$#" -eq 0 ] || [ "\${1:-}" = "--help" ] || [ "\${1:-}" = "-h" ]; then
  exec curl -fsS "$BRIDGE_URL/help"
fi

subcommand="$1"
shift

if [ "$#" -gt 0 ] && { [ "\${1:-}" = "--help" ] || [ "\${1:-}" = "-h" ]; }; then
  exec curl -fsS "$BRIDGE_URL/tools/$subcommand/help"
fi

args=(curl -fsS -X POST "$BRIDGE_URL/tools/$subcommand")
for arg in "$@"; do
  args+=(--data-urlencode "argv=$arg")
done
exec "\${args[@]}"
`;
}

function windowsScript(bridgeUrl: string): string {
  return `@echo off
setlocal
node -e "const bridgeUrl=${JSON.stringify(bridgeUrl)}; const argv=process.argv.slice(1); fetch(bridgeUrl + '/exec',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({argv})}).then(async (response)=>{const text=await response.text(); if (text) process.stdout.write(text.endsWith('\\n') ? text : text + '\\n'); if(!response.ok) process.exit(response.status || 1);}).catch((error)=>{console.error(error?.stack || String(error)); process.exit(1);});" -- %*
`;
}

function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}
