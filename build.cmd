@echo off
setlocal
rem Use the installed Rust toolchain; MSYS2 tools are available for GNU builds.
set "PATH=%USERPROFILE%\.cargo\bin;C:\msys64\mingw64\bin;%PATH%"
cd /d "%~dp0"
cargo build --release --locked
if errorlevel 1 exit /b 1
copy /y target\release\grids.exe grids.exe >nul
if errorlevel 1 exit /b 1
echo Built: %CD%\grids.exe
