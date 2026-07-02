$Root = Split-Path -Parent $PSScriptRoot
$Launcher = Join-Path $Root "MediaGen.exe"
$Icon = Join-Path $Root "icons\mediagen.ico"

$Shell = New-Object -ComObject WScript.Shell
$Desktop = [Environment]::GetFolderPath("Desktop")
$Shortcut = $Shell.CreateShortcut((Join-Path $Desktop "MediaGen.lnk"))
$Shortcut.TargetPath = $Launcher
$Shortcut.WorkingDirectory = $Root
$Shortcut.IconLocation = $Icon
$Shortcut.Save()
