@echo off
REM ============================================================
REM  한글일본어입력기 제거 (Windows)
REM ============================================================

echo.
echo  한글일본어입력기 제거
echo.

set APP_NAME=HangulJapaneseIME
set INSTALL_DIR=%APPDATA%\%APP_NAME%
set EXE_NAME=%APP_NAME%.exe

REM Kill process
echo  [1/3] 프로세스 종료...
taskkill /f /im %EXE_NAME% >nul 2>nul

REM Remove from startup
echo  [2/3] 시작 프로그램 제거...
set STARTUP_DIR=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
del /f /q "%STARTUP_DIR%\%EXE_NAME%" >nul 2>nul

REM Remove install dir
echo  [3/3] 파일 제거...
if exist "%INSTALL_DIR%" rmdir /s /q "%INSTALL_DIR%"

echo.
echo  제거 완료!
echo.
pause
