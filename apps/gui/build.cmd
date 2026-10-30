@echo off
setlocal
cd /d "%~dp0"
if not exist node_modules (
  echo Installing npm dependencies...
  call npm install --no-audit --no-fund || goto :error
)
echo Building Tauri app...
call npm run tauri build -- --debug --no-bundle || goto :error
if not exist "..\..\builds" mkdir "..\..\builds"
copy /Y "src-tauri\target\debug\aj199-gui.exe" "..\..\builds\aj199-gui-debug.exe" >nul
echo.
echo Build OK: builds\aj199-gui-debug.exe
pause
exit /b 0

:error
echo.
echo BUILD FAILED
pause
exit /b 1
