# Agent IDE Quick Start Guide 🚀

## One-Click Start (Recommended)

```powershell
npm run start
```

This command will:
1. Automatically check development environment
2. Provide fix options if Rust is not installed
3. Start the full application

## Quick Environment Fix

If you see `cargo: command not found` error:

### Method 1: Automatic Fix (Recommended)

```powershell
npm run fix:env
```

Or use batch version:

```powershell
npm run fix:env:bat
```

### Method 2: Manual Script

```powershell
# PowerShell version
.\fix-environment.ps1

# Or batch version
.\fix-environment.bat
```

### Method 3: Manual Rust Installation

```powershell
# Download Rust installer
Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile rustup-init.exe

# Run installer
.\rustup-init.exe

# Restart terminal (important!)

# Verify installation
rustc --version
cargo --version
```

## Development Mode Selection

### Full Mode (Requires Rust)

```powershell
npm run tauri -- dev
```

**Features:**
- ✅ Full desktop application
- ✅ File system operations
- ✅ Agent integration
- ✅ Terminal integration
- ✅ Git operations

### Web Preview Mode (No Rust Required)

```powershell
npm run dev
```

**Features:**
- ✅ UI/UX development
- ✅ Hot reload
- ✅ Frontend feature testing
- ❌ No file system access
- ❌ No Agent features
- ❌ No terminal integration

## Verify Installation

Run these commands to verify your environment:

```powershell
# Check Node.js
node --version    # Should show v18 or higher

# Check npm
npm --version     # Should show npm version

# Check Rust (required for full mode)
rustc --version   # Should show Rust version
cargo --version   # Should show Cargo version
```

## Quick Troubleshooting

| Issue | Quick Solution |
|-------|----------------|
| `cargo not found` | `npm run fix:env` |
| `node not found` | Install from https://nodejs.org/ |
| `port already in use` | Close program using port 1420 |
| `build failed` | `cargo clean && npm install` |
| `PATH issues` | Restart terminal or run `npm run fix:env` |

## Project Structure

```
agent-ide/
├── src/                    # Frontend source code
│   ├── components/         # React components
│   ├── hooks/             # React Hooks
│   ├── stores/            # State management
│   └── utils/             # Utility functions
├── src-tauri/            # Rust backend
│   └── src/              # Rust source code
├── docs/                 # Project documentation
├── fix-environment.ps1   # PowerShell environment fix script
├── fix-environment.bat   # Batch environment fix script
├── start.ps1             # PowerShell startup script
├── start.bat             # Batch startup script
└── README.md             # Project documentation
```

## Development Tips

1. **Frontend First**: Use `npm run dev` for UI development initially
2. **Smart Startup**: Use `npm run start` after environment fix
3. **Issue Resolution**: Check `RUST_FIX_GUIDE.md` for detailed guides
4. **Code Contribution**: Run `npm run build` and `cargo check` before committing

## Get Help

- 📖 **Full Documentation**: `README.md` and `README.zh-CN.md`
- 🛠️ **Detailed Guide**: `RUST_FIX_GUIDE.md`
- 🐛 **Bug Reports**: GitHub Issues
- 💬 **Discussions**: Project Discussions

## First Time User?

1. Run `npm run start` to launch the application
2. Follow prompts if environment issues are detected
3. Explore the interface and try Agent conversations
4. Check `docs/` directory for more features

Happy Coding! 🎉