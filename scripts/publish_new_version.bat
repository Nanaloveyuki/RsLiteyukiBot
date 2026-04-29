@echo off
setlocal EnableExtensions EnableDelayedExpansion

if "%~1"=="" (
  echo Usage: scripts\publish_new_version.bat ^<version^>
  echo Example: scripts\publish_new_version.bat 0.1.0
  exit /b 1
)

set "RAW_VERSION=%~1"
if /I "%RAW_VERSION:~0,1%"=="v" (
  set "TAG=%RAW_VERSION%"
) else (
  set "TAG=v%RAW_VERSION%"
)

git rev-parse --is-inside-work-tree >nul 2>&1
if errorlevel 1 (
  echo Error: current directory is not a git repository.
  exit /b 1
)

git remote get-url origin >nul 2>&1
if errorlevel 1 (
  echo Error: git remote 'origin' is not configured.
  exit /b 1
)

for /f %%I in ('git status --porcelain') do (
  echo Error: working tree is not clean. Commit or stash changes first.
  exit /b 1
)

git rev-parse -q --verify "refs/tags/%TAG%" >nul 2>&1
if not errorlevel 1 (
  echo Error: local tag '%TAG%' already exists.
  exit /b 1
)

for /f %%I in ('git ls-remote --tags origin "refs/tags/%TAG%"') do (
  echo Error: remote tag '%TAG%' already exists on origin.
  exit /b 1
)

echo Creating annotated tag %TAG% ...
git tag -a "%TAG%" -m "Release %TAG%"
if errorlevel 1 exit /b 1

echo Pushing tag %TAG% to origin ...
git push origin "%TAG%"
if errorlevel 1 exit /b 1

echo Done.
echo GitHub Actions will publish the image to ghcr.io/nanaloveyuki/rsliteyukibot-web:%TAG%
