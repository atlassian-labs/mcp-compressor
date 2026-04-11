import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const SCRIPT_MARKER = "mcp-compressor ts cli-mode script";
const BRIDGES_MARKER = "BRIDGES_JSON=";

type Bridges = Record<number, string>;

export async function findScriptDir(): Promise<{ dir: string; onPath: boolean }> {
  const pathDirs = new Set(
    (process.env.PATH ?? "")
      .split(path.delimiter)
      .filter(Boolean)
      .map((entry) => path.resolve(entry)),
  );
  const candidates =
    process.platform === "win32"
      ? [path.join(process.env.APPDATA ?? os.homedir(), "npm")]
      : [path.join(os.homedir(), ".local", "bin"), path.join(os.homedir(), "bin")];

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
  bridgePort: number,
  sessionPid: number,
  scriptDir?: string,
): Promise<{ scriptPath: string; onPath: boolean }> {
  const location = scriptDir ? { dir: scriptDir, onPath: true } : await findScriptDir();
  await fs.mkdir(location.dir, { recursive: true });
  const scriptPath = scriptPathFor(cliName, location.dir);

  const existing = await readBridges(scriptPath);
  const bridges = await liveBridges(existing);
  bridges[sessionPid] = `http://127.0.0.1:${bridgePort}`;

  if (process.platform === "win32") {
    await fs.writeFile(scriptPath, windowsScript(cliName, bridges), { encoding: "utf8" });
  } else {
    await fs.writeFile(scriptPath, unixScript(cliName, bridges), { encoding: "utf8", mode: 0o755 });
    await fs.chmod(scriptPath, 0o755);
  }

  return { scriptPath, onPath: location.onPath };
}

export async function removeCliScriptEntry(
  cliName: string,
  sessionPid: number,
  scriptDir?: string,
): Promise<void> {
  const location = scriptDir ? { dir: scriptDir } : { dir: (await findScriptDir()).dir };
  const scriptPath = scriptPathFor(cliName, location.dir);

  const existing = await readBridges(scriptPath);
  delete existing[sessionPid];
  const bridges = await liveBridges(existing);

  if (Object.keys(bridges).length === 0) {
    await removeCliScript(scriptPath);
    return;
  }

  if (process.platform === "win32") {
    await fs.writeFile(scriptPath, windowsScript(cliName, bridges), { encoding: "utf8" });
  } else {
    await fs.writeFile(scriptPath, unixScript(cliName, bridges), { encoding: "utf8", mode: 0o755 });
    await fs.chmod(scriptPath, 0o755);
  }
}

export async function removeCliScript(scriptPath: string): Promise<void> {
  try {
    const content = await fs.readFile(scriptPath, "utf8");
    if (!content.includes(SCRIPT_MARKER)) {
      return;
    }
    await fs.unlink(scriptPath);
  } catch {
    // ignore cleanup failures
  }
}

function scriptPathFor(cliName: string, scriptDir: string): string {
  return path.join(scriptDir, process.platform === "win32" ? `${cliName}.cmd` : cliName);
}

async function readBridges(scriptPath: string): Promise<Bridges> {
  try {
    const content = await fs.readFile(scriptPath, "utf8");
    const match = content.match(/BRIDGES_JSON=(.+)$/m);
    if (!match) {
      return {};
    }
    const parsed = JSON.parse(match[1] ?? "{}") as Record<string, string>;
    return Object.fromEntries(
      Object.entries(parsed)
        .map(([pid, url]) => [Number.parseInt(pid, 10), url] as const)
        .filter(([pid, url]) => Number.isInteger(pid) && typeof url === "string"),
    );
  } catch {
    return {};
  }
}

async function checkBridgeAlive(url: string): Promise<boolean> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 1000);
  try {
    const response = await fetch(`${url}/health`, { method: "GET", signal: controller.signal });
    return response.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

async function liveBridges(bridges: Bridges): Promise<Bridges> {
  const entries = await Promise.all(
    Object.entries(bridges).map(async ([pid, url]) =>
      (await checkBridgeAlive(url)) ? ([pid, url] as const) : null,
    ),
  );
  return Object.fromEntries(
    entries
      .filter((entry): entry is readonly [string, string] => entry !== null)
      .map(([pid, url]) => [Number.parseInt(pid, 10), url]),
  );
}

function bridgesJson(bridges: Bridges): string {
  return JSON.stringify(
    Object.fromEntries(Object.entries(bridges).sort(([a], [b]) => Number(a) - Number(b))),
  );
}

function bridgesBash(bridges: Bridges): string {
  return Object.entries(bridges)
    .sort(([a], [b]) => Number(a) - Number(b))
    .map(([pid, url]) => `  [${shellQuote(pid)}]=${shellQuote(url)}`)
    .join("\n");
}

function bridgesPowerShell(bridges: Bridges): string {
  return Object.entries(bridges)
    .sort(([a], [b]) => Number(a) - Number(b))
    .map(([pid, url]) => `${pid} = '${url.replace(/'/g, "''")}'`)
    .join("; ");
}

function unixScript(cliName: string, bridges: Bridges): string {
  const bridgesJsonValue = bridgesJson(bridges);
  const bridgesBashValue = bridgesBash(bridges);
  return `#!/usr/bin/env bash
# ${SCRIPT_MARKER}
# CLI name: ${cliName}
# Do not edit manually — managed by mcp-compressor
# ${BRIDGES_MARKER}${bridgesJsonValue}
set -euo pipefail

declare -A BRIDGES=(
${bridgesBashValue}
)

bridge_alive() {
  curl -fsS --max-time 1 "$1/health" >/dev/null 2>&1
}

find_bridge() {
  local pid="$$"
  while [ -n "$pid" ] && [ "$pid" -gt 1 ] 2>/dev/null; do
    if [ -n "\${BRIDGES[$pid]:-}" ]; then
      printf '%s' "\${BRIDGES[$pid]}"
      return 0
    fi
    pid="$(ps -o ppid= -p "$pid" | tr -d ' ')"
  done
  return 1
}

pick_bridge() {
  local url
  url="$(find_bridge || true)"
  if [ -n "$url" ] && bridge_alive "$url"; then
    printf '%s' "$url"
    return 0
  fi
  for url in "\${BRIDGES[@]}"; do
    if bridge_alive "$url"; then
      printf '%s' "$url"
      return 0
    fi
  done
  return 1
}

request() {
  local method="$1"
  shift
  local url="$1"
  shift
  local tmp
  tmp="$(mktemp)"
  local status
  if ! status="$(curl -sS -o "$tmp" -w '%{http_code}' -X "$method" "$url" "$@")"; then
    cat "$tmp" >&2 || true
    rm -f "$tmp"
    return 1
  fi
  cat "$tmp"
  rm -f "$tmp"
  case "$status" in
    2??) return 0 ;;
    4??|5??) return 1 ;;
    *) return 1 ;;
  esac
}

main() {
  local bridge
  bridge="$(pick_bridge || true)"
  if [ -z "$bridge" ]; then
    printf '%s\n%s\n' \
      'error: could not connect to mcp-compressor bridge' \
      'Is mcp-compressor running in --cli-mode?' >&2
    exit 1
  fi

  if [ "$#" -eq 0 ] || [ "\${1:-}" = "--help" ] || [ "\${1:-}" = "-h" ]; then
    request GET "$bridge/help" || exit 1
    exit 0
  fi

  local subcommand="$1"
  shift

  if [ "$#" -gt 0 ] && { [ "\${1:-}" = "--help" ] || [ "\${1:-}" = "-h" ]; }; then
    request GET "$bridge/tools/$subcommand/help" || exit 1
    exit 0
  fi

  local args=( )
  local arg
  for arg in "$@"; do
    args+=(--data-urlencode "argv=$arg")
  done
  request POST "$bridge/tools/$subcommand" "\${args[@]}" || exit 1
}

main "$@"
`;
}

function windowsScript(cliName: string, bridges: Bridges): string {
  const bridgesPs = bridgesPowerShell(bridges);
  return `@echo off
REM ${SCRIPT_MARKER}
REM CLI name: ${cliName}
REM Do not edit manually - managed by mcp-compressor
REM ${BRIDGES_MARKER}${bridgesJson(bridges)}
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$bridges = @{ ${bridgesPs} }; " ^
  "$bridge = $null; " ^
  "try { $proc = Get-Process -Id $PID; while ($proc.Parent -ne $null) { $proc = $proc.Parent; if ($bridges.ContainsKey($proc.Id)) { $bridge = $bridges[$proc.Id]; break } } } catch {} ; " ^
  "if (-not $bridge) { foreach ($u in $bridges.Values) { try { if ((Invoke-WebRequest -Uri ($u+'/health') -UseBasicParsing -TimeoutSec 1 -ErrorAction Stop).StatusCode -eq 200) { $bridge = $u; break } } catch {} } } ; " ^
  "if (-not $bridge) { Write-Error 'error: could not connect to mcp-compressor bridge'; Write-Error 'Is mcp-compressor running in --cli-mode?'; exit 1 } ; " ^
  "$argvList = @(%*); " ^
  "if ($argvList.Count -eq 0 -or $argvList[0] -eq '--help' -or $argvList[0] -eq '-h') { $r = Invoke-WebRequest -Uri ($bridge+'/help') -UseBasicParsing -ErrorAction Stop; Write-Host $r.Content; exit 0 } ; " ^
  "$subcommand = $argvList[0]; $rest = @(); if ($argvList.Count -gt 1) { $rest = $argvList[1..($argvList.Count-1)] } ; " ^
  "if ($rest.Count -gt 0 -and ($rest[0] -eq '--help' -or $rest[0] -eq '-h')) { $r = Invoke-WebRequest -Uri ($bridge+'/tools/'+$subcommand+'/help') -UseBasicParsing -ErrorAction Stop; Write-Host $r.Content; exit 0 } ; " ^
  "$pairs = New-Object System.Collections.Generic.List[string]; foreach ($arg in $rest) { $pairs.Add('argv=' + [System.Uri]::EscapeDataString([string]$arg)) } ; " ^
  "$body = [string]::Join('&', $pairs); " ^
  "try { $r = Invoke-WebRequest -Uri ($bridge+'/tools/'+$subcommand) -Method POST -ContentType 'application/x-www-form-urlencoded' -Body $body -UseBasicParsing -ErrorAction Stop; Write-Host $r.Content; exit 0 } catch { $e = $_.Exception; if ($e -is [System.Net.WebException] -and $e.Response) { $sr = New-Object System.IO.StreamReader($e.Response.GetResponseStream()); Write-Error $sr.ReadToEnd(); exit 1 }; Write-Error ('error: ' + $e.Message); exit 1 }"
`;
}

function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}
