# Rust 环境问题快速解决方案

## 问题诊断

当你运行 `npm run tauri -- dev` 时遇到以下错误：

```
failed to run 'cargo metadata' command to get workspace directory: failed to run command cargo metadata --no-deps --format-version 1: program not found
```

这表明系统找不到 `cargo` 命令，需要安装或配置 Rust 环境。

## 快速修复步骤

### 方案 1：自动修复脚本（推荐）

我们提供了两个自动修复脚本：

**PowerShell 版本（推荐）：**
```powershell
.\fix-environment.ps1
```

**批处理版本（备选）：**
```powershell
.\fix-environment.bat
```

这些脚本会自动：
- 检查 Node.js 和 npm 安装
- 检查 Rust 和 Cargo 安装
- 自动下载并安装 Rust（如果需要）
- 修复 PATH 配置
- 验证安装

### 方案 2：手动安装 Rust

#### Windows

1. **下载 Rust 安装程序**：
   ```powershell
   Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile rustup-init.exe
   ```

2. **运行安装程序**：
   ```powershell
   .\rustup-init.exe
   ```

3. **重启终端**（重要！）

4. **验证安装**：
   ```powershell
   rustc --version
   cargo --version
   ```

#### macOS/Linux

1. **运行安装命令**：
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **重新加载环境**：
   ```bash
   source $HOME/.cargo/env
   ```

3. **验证安装**：
   ```bash
   rustc --version
   cargo --version
   ```

### 方案 3：修复 PATH 问题

如果 Rust 已安装但 `cargo` 命令仍然找不到：

#### Windows PowerShell

**临时修复（当前会话）：**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin"
cargo --version  # 现在应该可以工作
```

**永久修复：**
```powershell
# 添加到用户环境变量
[Environment]::SetEnvironmentVariable(
    "Path",
    [Environment]::GetEnvironmentVariable("Path", "User") + ";$env:USERPROFILE\.cargo\bin",
    "User"
)

# 重启终端使更改生效
```

#### macOS/Linux

**修复 shell 配置：**
```bash
# 对于 bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc

# 对于 zsh (macOS 默认)
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

## 验证修复

修复后，运行以下命令验证：

```powershell
# 1. 验证 Rust 安装
rustc --version
cargo --version

# 2. 验证 Node.js
node --version
npm --version

# 3. 测试 Tauri 构建
npm run tauri -- dev
```

## 常见问题

### Q: 安装很慢或下载失败

**A:** Rust 服务器可能在你的地区访问较慢，尝试：

```powershell
# 使用国内镜像（中国用户）
$env:RUSTUP_DIST_SERVER = "https://mirrors.tuna.tsinghua.edu.cn/rustup"
.\rustup-init.exe
```

### Q: 权限错误

**A:** Windows 上以管理员身份运行 PowerShell：

```powershell
# 右键点击 PowerShell -> "以管理员身份运行"
```

### Q: 仍然找不到 cargo

**A:** 完全重启计算机，然后：
1. 打开新的终端窗口
2. 运行 `cargo --version`
3. 如果仍然失败，手动添加 PATH：
   - 右键 "此电脑" -> 属性 -> 高级系统设置 -> 环境变量
   - 在 "用户变量" 中找到 "Path"，点击编辑
   - 添加新条目：`%USERPROFILE%\.cargo\bin`

### Q: 首次运行很慢

**A:** 这是正常的！首次构建需要：
- 下载 Rust 编译器
- 编译所有依赖
- 可能需要 5-15 分钟

后续运行会快很多。

## 开发模式选择

### 如果 Rust 环境问题暂时无法解决

你可以先使用 Web 预览模式进行前端开发：

```powershell
npm run dev
```

这个模式：
- ✅ 不需要 Rust 环境
- ✅ 可以进行 UI/UX 开发
- ✅ 支持热重载
- ❌ 没有文件系统访问
- ❌ 没有 Agent 功能
- ❌ 没有终端集成

### 完整开发模式

当 Rust 环境配置好后：

```powershell
npm run tauri -- dev
```

这个模式：
- ✅ 完整的桌面应用
- ✅ 所有功能可用
- ✅ 文件系统操作
- ✅ Agent 集成

## 获取更多帮助

如果以上方法都无法解决：

1. **查看完整文档**：README.md 和 README.zh-CN.md
2. **检查 Rust 官方文档**：https://www.rust-lang.org/tools/install
3. **查看 Tauri 文档**：https://tauri.app/v1/guides/
4. **提交 GitHub Issue**，包含：
   - 操作系统版本
   - 错误信息完整截图
   - 已尝试的解决方法
   - `rustc --version` 和 `cargo --version` 的输出

## 环境检查清单

在提交问题前，请确认：

- [ ] 已重启终端
- [ ] 已重启计算机（如果 PATH 修改）
- [ ] `cargo --version` 可以正常运行
- [ ] `node --version` 显示 v18 或更高
- [ ] 已运行 `npm install`
- [ ] 已查看项目 README 中的故障排除部分
- [ ] 已尝试此文档中的所有解决方案

## 下一步

环境修复后，你可以：

1. **启动开发服务器**：
   ```powershell
   npm run tauri -- dev
   ```

2. **查看项目文档**：
   - `README.md` - 项目概览
   - `ROADMAP.md` - 开发路线图
   - `docs/agent_ide_design.md` - 技术设计

3. **开始开发**：
   - 查看 `src/components/` 目录了解组件结构
   - 查看 `src-tauri/src/` 了解后端架构
   - 参考 `docs/smoke_test.md` 了解测试方法

祝开发愉快！ 🚀