@echo off
echo ============================================
echo   VMQ MVP - Windows Setup Script
echo   Run this as Administrator!
echo ============================================
echo.

:: Check for admin privileges
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo ERROR: This script must be run as Administrator.
    echo Right-click the script and select "Run as administrator".
    pause
    exit /b 1
)

echo [1/4] Opening port 80 in Windows Firewall...
netsh advfirewall firewall add rule name="VMQ MVP HTTP (Port 80)" dir=in action=allow protocol=TCP localport=80
echo.

echo [2/4] Opening port 443 in Windows Firewall (for Cloudflare)...
netsh advfirewall firewall add rule name="VMQ MVP HTTPS (Port 443)" dir=in action=allow protocol=TCP localport=443
echo.

echo [3/4] Creating audio storage directory...
mkdir "E:\vmq_data\audio" 2>nul
if exist "E:\vmq_data\audio" (
    echo Directory E:\vmq_data\audio is ready.
) else (
    echo WARNING: Could not create E:\vmq_data\audio - please create it manually.
)
echo.

echo [4/4] Verifying .secrets\api_key.env exists...
if exist ".secrets\api_key.env" (
    echo API key file found.
) else (
    echo WARNING: .secrets\api_key.env not found!
    echo Create it with: echo GEMINI_API_KEY=your_key_here > .secrets\api_key.env
)
echo.

echo ============================================
echo   Setup complete!
echo.
echo   To run the app:
echo     dx serve --platform web
echo.
echo   Or for a release build:
echo     dx build --platform web --release
echo     .\target\release\vmq_mvp.exe
echo.
echo   The app will listen on http://0.0.0.0:80
echo   Override with: set IP=0.0.0.0 ^& set PORT=8080
echo ============================================
pause
