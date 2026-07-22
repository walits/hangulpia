@echo off
REM ============================================================
REM  한글일본어입력기 설치 (Windows)
REM ============================================================

echo.
echo  ========================================
echo   한글일본어입력기 설치
echo  ========================================
echo.

set APP_NAME=HangulJapaneseIME
set INSTALL_DIR=%APPDATA%\%APP_NAME%
set EXE_NAME=%APP_NAME%.exe
set SCRIPT_DIR=%~dp0

REM Check exe exists
if not exist "%SCRIPT_DIR%%EXE_NAME%" (
    echo  [오류] %EXE_NAME% 을 찾을 수 없습니다.
    echo         이 스크립트와 같은 폴더에 있어야 합니다.
    pause
    exit /b 1
)

REM Kill existing process
echo  [1/4] 기존 프로세스 정리...
taskkill /f /im %EXE_NAME% >nul 2>nul

REM Create install directory
echo  [2/4] 설치 중...
if not exist "%INSTALL_DIR%" mkdir "%INSTALL_DIR%"
copy /y "%SCRIPT_DIR%%EXE_NAME%" "%INSTALL_DIR%\" >nul

REM Add to startup (current user)
echo  [3/4] 시작 프로그램 등록...
set STARTUP_DIR=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
copy /y "%SCRIPT_DIR%%EXE_NAME%" "%STARTUP_DIR%\" >nul 2>nul

REM Launch
echo  [4/4] 실행...
start "" "%INSTALL_DIR%\%EXE_NAME%"

echo.
echo  ========================================
echo   설치 완료!
echo  ========================================
echo.
echo   사용법:
echo     Ctrl+Space  입력기 켜기/끄기
echo     한글 자판으로 입력하면 히라가나로 변환됩니다.
echo     스페이스/엔터로 확정
echo.
echo   예시:
echo     dkflrkxh (아리가토) → ありがと
echo.
echo   제거: uninstall.bat 실행
echo.
pause
