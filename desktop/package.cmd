@echo off
setlocal

set "GIT_BASH="
if exist "%ProgramFiles%\Git\bin\bash.exe" set "GIT_BASH=%ProgramFiles%\Git\bin\bash.exe"
if not defined GIT_BASH if exist "%ProgramFiles(x86)%\Git\bin\bash.exe" set "GIT_BASH=%ProgramFiles(x86)%\Git\bin\bash.exe"
if not defined GIT_BASH if exist "%LocalAppData%\Programs\Git\bin\bash.exe" set "GIT_BASH=%LocalAppData%\Programs\Git\bin\bash.exe"
if not defined GIT_BASH (
  for /f "delims=" %%I in ('where git.exe 2^>nul') do (
    if not defined GIT_BASH if exist "%%~dpI..\bin\bash.exe" set "GIT_BASH=%%~dpI..\bin\bash.exe"
  )
)

if not defined GIT_BASH (
  echo error: Git for Windows ^(bash.exe^) not found. Install Git for Windows first. 1>&2
  exit /b 1
)

"%GIT_BASH%" "%~dp0..\dev.sh" build desktop %*
exit /b %ERRORLEVEL%
