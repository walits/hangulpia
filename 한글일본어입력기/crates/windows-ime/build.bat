@echo off
REM ============================================================
REM  한글일본어입력기 (Windows) 빌드 스크립트
REM
REM  요구 사항:
REM    - Rust (rustup.rs 에서 설치)
REM    - Visual Studio Build Tools (C++ 빌드 도구)
REM
REM  사용법: build.bat
REM ============================================================

echo.
echo  ========================================
echo   한글일본어입력기 Windows 빌드
echo  ========================================
echo.

REM Check cargo
where cargo >nul 2>nul
if %errorlevel% neq 0 (
    echo  [오류] cargo를 찾을 수 없습니다.
    echo         https://rustup.rs 에서 Rust를 설치하세요.
    exit /b 1
)

REM Build release
echo  [1/3] 빌드 중...
cd /d "%~dp0\..\.."
cargo build --release -p ime-windows
if %errorlevel% neq 0 (
    echo  [오류] 빌드 실패
    exit /b 1
)
echo  [완료] 빌드 성공

REM Create dist folder
echo  [2/3] 패키지 생성...
set DIST_DIR=%~dp0\dist\HangulJapaneseIME-Windows
if exist "%DIST_DIR%" rmdir /s /q "%DIST_DIR%"
mkdir "%DIST_DIR%"

copy "target\release\HangulJapaneseIME.exe" "%DIST_DIR%\"
copy "%~dp0\install.bat" "%DIST_DIR%\"
copy "%~dp0\uninstall.bat" "%DIST_DIR%\"
copy "%~dp0\README-Windows.txt" "%DIST_DIR%\"

echo  [3/3] 완료!
echo.
echo  배포 폴더: %DIST_DIR%
echo  이 폴더를 zip으로 압축해서 테스터에게 전달하세요.
echo.
