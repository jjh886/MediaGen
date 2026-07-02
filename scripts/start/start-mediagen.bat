@echo off
setlocal
set ROOT=%~dp0
set SERVER=%ROOT%\bin\mediagen-server.exe

if not exist "%SERVER%" (
  echo 没有找到 mediagen-server：%SERVER%
  pause
  exit /b 1
)

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0start-mediagen.ps1"
