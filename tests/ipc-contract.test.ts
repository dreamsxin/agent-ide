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

function rustSources(): string[] {
    const files = readdirSync(join("src-tauri", "src"), {
        recursive: true,
        encoding: "utf8",
    }).filter((file: string) => file.endsWith(".rs"));
    return files.map((file: string) => readFileSync(join("src-tauri", "src", file), "utf8"));
}

/**
 * 后端发出的事件名。
 *
 * 必须按整个文件文本匹配而不是逐行：一多半的 `emit` 调用是
 * `emit(\n    "agent-pipeline-update",` 这种换行写法，按行扫会漏掉 12 个里的 7 个。
 * 这个坑本身就说明为什么这道接缝值得测 —— 名字是字符串，谁都不会替你检查。
 */
function emittedEvents(): Set<string> {
    const names = new Set<string>();
    for (const source of rustSources()) {
        for (const match of source.matchAll(/\bemit(?:_json|_all|_to)?\(\s*"([a-z0-9-]+)"/g)) {
            names.add(match[1]);
        }
    }
    return names;
}

/** 前端监听的事件名 -> 第一个监听它的文件 */
function listenedEvents(): Map<string, string> {
    const listened = new Map<string, string>();
    for (const file of frontendSources()) {
        const source = readFileSync(file, "utf8");
        for (const match of source.matchAll(/\blisten(?:<[\s\S]*?>)?\(\s*"([a-z0-9-]+)"/g)) {
            if (!listened.has(match[1])) {
                listened.set(match[1], file);
            }
        }
    }
    return listened;
}

function frontendSources(): string[] {
    const files = readdirSync("src", { recursive: true, encoding: "utf8" }).filter(
        (file: string) => file.endsWith(".ts") || file.endsWith(".tsx")
    );
    return files.map((file: string) => join("src", file));
}

function invokedCommands(): Map<string, string> {
    const invoked = new Map<string, string>();

    for (const path of frontendSources()) {
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

/**
 * 事件名和命令名是同一类东西：字符串，两边都没有编译器把关。区别只在失败的样子 ——
 * 命令名写错会抛运行时错误，事件名写错**什么都不会发生**。界面就那样静静地不再更新，
 * 而后端日志里一切正常。这是最难靠手点发现的一种坏法。
 *
 * 两个方向都查，因为两种漂移都真实存在：只改了发送端 → 界面停更；只改了监听端 →
 * 后端白算一场。当前两个集合正好完全重合，所以不需要豁免名单 —— 以后谁要加一个
 * 只给 CLI 用的事件，会被这条测试逼着说明理由，这正是想要的摩擦。
 */
describe("Tauri event contract", () => {
    it("emits every event the frontend listens for", () => {
        const emitted = emittedEvents();
        const listened = listenedEvents();

        expect(emitted.size).toBeGreaterThan(8);
        expect(listened.size).toBeGreaterThan(8);

        const orphanListeners = [...listened.entries()]
            .filter(([name]) => !emitted.has(name))
            .map(([name, file]) => `${name} (listened in ${file})`);

        expect(
            orphanListeners,
            "nothing in src-tauri emits these — the UI would silently never update"
        ).toEqual([]);
    });

    it("has a listener for every event it emits", () => {
        const emitted = emittedEvents();
        const listened = listenedEvents();

        const unheard = [...emitted].filter((name) => !listened.has(name));

        expect(unheard, "these events are emitted but nobody listens — wasted work").toEqual([]);
    });

    /**
     * 解析器自身的回归护栏：`emit` 的多行写法占了一多半，按行匹配会漏掉它们。
     * 漏掉之后上面第一条会红，但报出来的原因会指向"前端监听了不存在的事件"，
     * 把人带到错误的方向去。所以直接钉住一个已知的多行发送点。
     */
    it("finds emit calls whose name is on the next line", () => {
        expect(emittedEvents().has("agent-pipeline-update")).toBe(true);
    });

    /**
     * 监听端的对称风险：泛型参数里带花括号（`listen<{ status: LspStatus }>`）时，
     * 一个偷懒的 `<[^>]*>` 会在第一个 `>` 处断掉，于是这个监听者被无声跳过 ——
     * 上面两条都还会绿，因为它压根没进集合。
     */
    it("finds listeners whose type argument contains braces", () => {
        expect(listenedEvents().has("lsp-status")).toBe(true);
        expect(listenedEvents().has("terminal-output")).toBe(true);
    });
});

