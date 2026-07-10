$ErrorActionPreference = "Stop"

$Root = $PSScriptRoot
if (!(Test-Path (Join-Path $Root "bin\mediagen-server.exe"))) {
  $Candidate = Resolve-Path (Join-Path $PSScriptRoot "..\..") -ErrorAction SilentlyContinue
  if ($Candidate -and (Test-Path (Join-Path $Candidate "bin\mediagen-server.exe"))) {
    $Root = "$Candidate"
  }
}

$Server = Join-Path $Root "bin\mediagen-server.exe"
if (!(Test-Path $Server)) {
  Write-Host "没有找到 mediagen-server：$Server"
  Read-Host "按回车退出"
  exit 1
}

$DataRoot = if ($env:MEDIAGEN_DATA_DIR) {
  $env:MEDIAGEN_DATA_DIR
} elseif ($env:LOCALAPPDATA) {
  Join-Path $env:LOCALAPPDATA "MediaGen"
} else {
  Join-Path $env:USERPROFILE ".mediagen"
}
$LogsRoot = if ($env:MEDIAGEN_LOGS_DIR) {
  $env:MEDIAGEN_LOGS_DIR
} else {
  Join-Path $DataRoot "logs"
}
$ProfilesRoot = if ($env:MEDIAGEN_PROFILES_DIR) {
  $env:MEDIAGEN_PROFILES_DIR
} else {
  Join-Path $DataRoot "profiles"
}

New-Item -ItemType Directory -Force -Path $DataRoot | Out-Null
New-Item -ItemType Directory -Force -Path $LogsRoot | Out-Null
New-Item -ItemType Directory -Force -Path $ProfilesRoot | Out-Null
$Log = Join-Path $LogsRoot "startup.log"

function Test-Healthy([int]$Port) {
  try {
    Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$Port/health" | Out-Null
    return $true
  } catch {
    return $false
  }
}

$Port = if ($env:MEDIAGEN_PORT) { [int]$env:MEDIAGEN_PORT } else { 0 }
if ($Port -eq 0) {
  foreach ($CandidatePort in 18080..18089) {
    $Open = Test-NetConnection -ComputerName 127.0.0.1 -Port $CandidatePort -InformationLevel Quiet
    if ((Test-Healthy $CandidatePort) -or !$Open) {
      $Port = $CandidatePort
      break
    }
  }
}

if ($Port -eq 0) {
  Write-Host "端口 18080-18089 都被占用，请先关闭旧的 MediaGen 或其他占用端口的程序"
  Read-Host "按回车退出"
  exit 1
}

if (!$env:CLOAK_BROWSER_PATH) {
  $Bundled = @(
    (Join-Path $Root "browser-runtime\chrome.exe"),
    (Join-Path $Root "browser-runtime\CloakBrowser\chrome.exe"),
    (Join-Path $Root "browser-runtime\CloakBrowser\Application\cloakbrowser.exe"),
    (Join-Path $Root "browser-runtime\cloakbrowser.exe")
  ) | Where-Object { Test-Path $_ } | Select-Object -First 1
  if ($Bundled) {
    $env:CLOAK_BROWSER_PATH = $Bundled
  }
}

if (!(Test-Healthy $Port)) {
  $env:MEDIAGEN_PORT = "$Port"
  $env:MEDIAGEN_DATA_DIR = $DataRoot
  $env:MEDIAGEN_LOGS_DIR = $LogsRoot
  $env:MEDIAGEN_PROFILES_DIR = $ProfilesRoot
  $env:MEDIAGEN_PLUGINS_DIR = Join-Path $Root "plugins"
  $env:MEDIAGEN_CONSOLE_DIR = Join-Path $Root "console"
  Start-Process -FilePath $Server -WorkingDirectory $Root -WindowStyle Hidden -RedirectStandardOutput $Log -RedirectStandardError $Log
}

for ($i = 0; $i -lt 150; $i++) {
  if (Test-Healthy $Port) {
    Start-Process "http://127.0.0.1:$Port"
    exit 0
  }
  Start-Sleep -Milliseconds 300
}

Write-Host "MediaGen 启动超时，请查看 $Log"
Read-Host "按回车退出"
exit 1
