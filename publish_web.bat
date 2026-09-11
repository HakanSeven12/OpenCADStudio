@echo off
setlocal

rem Double-click to build and publish. Use --no-pause from a terminal or CI.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\publish-web.ps1"
set "PUBLISH_EXIT=%ERRORLEVEL%"
if not "%PUBLISH_EXIT%"=="0" echo Web CAD publication failed. See the error above.
if /I not "%~1"=="--no-pause" pause
exit /b %PUBLISH_EXIT%
