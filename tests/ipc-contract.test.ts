import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

/**
 * IPC 名字是字符串，两边都没有编译器把关。
 *
 * 前端写错一个字（比如 verify_workspce）既不会被 tsc 发现，也不会被 cargo
 * 发现 —— 只有用户点到那个按钮时才会以运行时报错的形式冒出来。本轮会话新增了
 * 五个命令，这类失误正是"只能靠手点界面才能发现"的典型，所以用测试把这道接缝锁住。
 *
 * 这个文件放在 `tests/` 而不是 `src/`：它要读文件系统，而 `tsconfig.json` 刻意
 * 只给应用代码 `vite/client` 类型、不给 Node 类型。为一个工具型测试去放开应用
 * 代码的类型边界不值得。
 */
function registeredCommands(): Set<string> {
    const libSource = readFileSync(join("src-tauri", "src", "lib.rs"), "utf8");
    const start = libSource.indexOf("generate_handler![");
    expect(start, "generate_handler! block not found in lib.rs").toBeGreaterThan(-1);
    const end = libSource.indexOf("])", start);
    const block = libSource.slice(start, end);

    const names = new Set<string>();
    for (const match of block.matchAll(/commands::[a-z_]+::([a-z0-9_]+)/g)) {
        names.add(match[1]);
    }
    return names;
}

function invokedCommands(): Map<string, string> {
    const invoked = new Map<string, string>();
    const files = readdirSync("src", { recursive: true, encoding: "utf8" }).filter(
        (file: string) => file.endsWith(".ts") || file.endsWith(".tsx")
    );

    for (const file of files) {
        const path = join("src", file);
        const source = readFileSync(path, "utf8");
        // invoke("name") 以及 invoke<T>("name")
        for (const match of source.matchAll(/\binvoke(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g)) {
            if (!invoked.has(match[1])) {
                invoked.set(match[1], path);
            }
        }
    }
    return invoked;
}

describe("Tauri IPC contract", () => {
    it("registers every command the frontend invokes", () => {
        const registered = registeredCommands();
        const invoked = invokedCommands();

        // 前提检查：解析本身必须有效，否则这条测试会以"全部通过"的方式静默失效
        expect(registered.size).toBeGreaterThan(20);
        expect(invoked.size).toBeGreaterThan(10);

        const missing = [...invoked.entries()]
            .filter(([name]) => !registered.has(name))
            .map(([name, file]) => `${name} (invoked in ${file})`);

        expect(missing, "these commands are invoked but not registered in lib.rs").toEqual([]);
    });

    it("parses the whole handler list, not just the beginning", () => {
        const registered = registeredCommands();

        // 抽查最近新增的几个：它们在列表末尾，能证明解析覆盖到了结尾
        for (const name of [
            "undo_last_apply",
            "verify_workspace",
            "agent_repair_prompt",
            "clear_agent_conversation",
        ]) {
            expect(registered.has(name), `${name} should be registered`).toBe(true);
        }
    });
});
