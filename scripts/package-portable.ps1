$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path "portable\ClipAnchor\data" | Out-Null
Copy-Item -Recurse -Force "src-tauri\target\release\bundle\*" "portable\ClipAnchor" -ErrorAction SilentlyContinue
Write-Host "Portable package prepared under portable\ClipAnchor"
