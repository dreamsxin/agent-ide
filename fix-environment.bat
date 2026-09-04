@echo off
REM Agent IDE 环境诊断和修复工具 (批处理)
REM 用于诊断和修复常见的 Rust/Tauri 环境问题

echo ============================================================
echo Agent IDE 环境诊断和修复工具
echo ============================================================
echo.

REM 1. 检查 Node.js 和 npm
echo [1/5] 检查 Node.js 和 npm...
node --version >nul 2>&1
if %errorlevel% neq 0 (
    echo [错误] Node.js 未安装
    echo 请从 https://nodejs.org/ 下载并安装 Node.js 18 或更高版本
    goto :end
) else (
    echo [√] Node.js 已安装:
    node --version
    echo [√] npm 已安装:
    npm --version
)
echo.

REM 2. 检查 Rust 和 Cargo
echo [2/5] 检查 Rust 和 Cargo...
cargo --version >nul 2>&1
if %errorlevel% neq 0 (
    echo [错误] Rust/Cargo 未找到
    echo.
    echo 正在启动 Rust 安装程序...
    echo.

    REM 检查是否已有 rustup 安装器
    if exist "rustup-init.exe" (
        echo [发现] 已找到 rustup-init.exe，正在运行...
        rustup-init.exe -y --default-toolchain stable
    ) else (
        echo [下载] 正在下载 Rust 安装程序...
        powershell -Command "Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile rustup-init.exe"
        if %errorlevel% neq 0 (
            echo [错误] 下载失败
            echo 请手动访问 https://rustup.rs/ 下载安装程序
            goto :end
        )
        echo [安装] 正在安装 Rust...
        rustup-init.exe -y --default-toolchain stable
    )

    echo.
    echo [重要] 请关闭当前终端并重新打开以使 PATH 变更生效
    echo 然后重新运行此脚本进行验证
    goto :end
) else (
    echo [√] Rust 已安装:
    rustc --version
    echo [√] Cargo 已安装:
    cargo --version
)
echo.

REM 3. 检查 Cargo 是否在 PATH 中
echo [3/5] 检查 Cargo PATH 配置...
where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo [警告] Cargo 不在系统 PATH 中
    echo.
    echo 正在修复 PATH 配置...

    for /f "tokens=2*" %%a in ('reg query "HKCU\Environment" /v Path 2^>nul') do set "user_path=%%b"

    if defined user_path (
        echo %user_path% | findstr /C:".cargo\bin" >nul
        if %errorlevel% neq 0 (
            echo [添加] 将 Cargo bin 目录添加到用户 PATH
            setx Path "%user_path%;%USERPROFILE%\.cargo\bin" >nul
            echo [完成] PATH 已更新，请重启终端使更改生效
            set path_fixed=1
        ) else (
            echo [√] Cargo bin 目录已在 PATH 中
        )
    ) else (
        echo [添加] 创建用户 PATH 并添加 Cargo bin 目录
        setx Path "%USERPROFILE%\.cargo\bin" >nul
        echo [完成] PATH 已设置，请重启终端使更改生效
        set path_fixed=1
    )
) else (
    echo [√] Cargo 在 PATH 中配置正确
)
echo.

REM 4. 检查 Tauri CLI
echo [4/5] 检查 Tauri CLI...
if not exist "node_modules\.bin\tauri.cmd" (
    echo [警告] Tauri CLI 未安装
    echo 正在安装依赖...
    call npm install
    if %errorlevel% neq 0 (
        echo [错误] npm install 失败
        goto :end
    )
    echo [√] 依赖安装完成
) else (
    echo [√] Tauri CLI 已安装
)
echo.

REM 5. 测试构建
echo [5/5] 测试 Tauri 构建...
echo [信息] 运行快速构建测试...
cd src-tauri
cargo check --quiet 2>nul
if %errorlevel% neq 0 (
    echo [警告] cargo check 失败，但这可能是正常的（首次构建可能需要更长时间）
    echo [建议] 运行 'npm run tauri -- dev' 进行完整测试
) else (
    echo [√] Rust 代码编译通过
)
cd ..
echo.

echo ============================================================
echo 诊断完成
echo ============================================================
echo.

if defined path_fixed (
    echo [重要] 检测到 PATH 已更新
    echo 请按以下步骤操作：
    echo 1. 关闭当前终端
    echo 2. 重新打开终端
    echo 3. 运行 'npm run tauri -- dev'
    echo.
) else (
    echo 下一步操作：
    echo 1. 运行 'npm run tauri -- dev' 启动应用
    echo 2. 如果仍有问题，请查看 README.md 中的故障排除部分
    echo.
)

:end
pause