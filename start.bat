@echo off
REM Agent IDE 启动脚本
REM 智能检测环境并选择合适的启动方式

echo ============================================================
echo Agent IDE 启动器
echo ============================================================
echo.

REM 检查环境
echo 检查开发环境...

REM 检查 Node.js
where node >nul 2>&1
if %errorlevel% neq 0 (
    echo [错误] Node.js 未安装
    echo 请从 https://nodejs.org/ 下载并安装 Node.js
    pause
    exit /b 1
)

REM 检查 Cargo
where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo [警告] Rust/Cargo 未找到
    echo.
    echo 选择启动方式：
    echo 1. 运行环境修复脚本（推荐）
    echo 2. 启动 Web 预览模式（无需 Rust）
    echo 3. 手动修复后重试
    echo.
    set /p choice="请输入选择 (1/2/3): "

    if "%choice%"=="1" (
        echo.
        echo 运行环境修复脚本...
        call fix-environment.bat
        echo.
        echo 修复完成后，请重新运行此脚本
        pause
        exit /b 0
    )

    if "%choice%"=="2" (
        echo.
        echo 启动 Web 预览模式...
        echo 注意：此模式不支持文件系统、Agent、终端等功能
        echo.
        npm run dev
        exit /b 0
    )

    if "%choice%"=="3" (
        echo.
        echo 请参考 RUST_FIX_GUIDE.md 修复环境问题
        pause
        exit /b 0
    )

    echo 无效选择，退出
    pause
    exit /b 1
)

REM 环境正常，启动完整应用
echo [√] 环境检查通过
echo.
echo 启动 Agent IDE (完整模式)...
echo.

npm run tauri -- dev