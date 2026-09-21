import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import { normalizeDestructiveOpType } from "../src/types/agent";

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
 * `pub const SOMETHING: &str = "…";` 形式的字符串常量。
 *
 * 存在的理由：批准机制的事件名被后端和它自己的测试同时引用，写成常量是对的 —— 但那让
 * 扫字面量的解析器看不见它们，于是这个文件报出"前端监听了一个没人发的事件"。修解析器
 * 而不是把名字抄成两份字面量：抄两份正是这个文件要防的漂移。
 *
 * 同一个映射也给下面的 `opType` 用：那条测试第一版只认字面量，等于把这里的教训重犯
 * 一遍 —— 一个新动作把 op 类型写成常量就会从检查里消失。
 */
function stringConstants(sources: string[]): Map<string, string> {
    const constants = new Map<string, string>();
    for (const source of sources) {
        for (const match of source.matchAll(
            /const\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([a-z0-9_-]+)"/g
        )) {
            constants.set(match[1], match[2]);
        }
    }
    return constants;
}

/**
 * 从 Rust 源码里收集一类名字：字面量形式 + 常量形式。
 *
 * 两处调用（事件名、批准的 opType）共用这一处：只认字面量的扫描器会在名字被提成常量的
 * 那天静默失效，而失效的表现是"少了一个名字"，第一个报错的测试会把原因指向前端。
 */
function namesFrom(
    sources: string[],
    constants: Map<string, string>,
    literal: RegExp,
    named: RegExp
): Set<string> {
    const names = new Set<string>();
    for (const source of sources) {
        for (const match of source.matchAll(literal)) {
            names.add(match[1]);
        }
        for (const match of source.matchAll(named)) {
            const resolved = constants.get(match[1]);
            if (resolved) {
                names.add(resolved);
            }
        }
    }
    return names;
}

/**
 * 后端发出的事件名。
 *
 * 必须按整个文件文本匹配而不是逐行：一多半的 `emit` 调用是
 * `emit(\n    "agent-pipeline-update",` 这种换行写法，按行扫会漏掉 12 个里的 7 个。
 * 这个坑本身就说明为什么这道接缝值得测 —— 名字是字符串，谁都不会替你检查。
 */
function emittedEvents(): Set<string> {
    const sources = rustSources();
    return namesFrom(
        sources,
        stringConstants(sources),
        /\bemit(?:_json|_all|_to)?\(\s*"([a-z0-9-]+)"/g,
        /\bemit(?:_json|_all|_to)?\(\s*([A-Z][A-Z0-9_]*)\s*,/g
    );
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
     * 常量形式的发送点同样要被看见。没有这条的话，把某个事件名从字面量改成常量
     * 会让它悄悄从"已发出"集合里消失，而第一条测试报出来的原因会指向前端。
     */
    it("resolves event names that are declared as constants", () => {
        expect(emittedEvents().has("agent-approval-requested")).toBe(true);
        expect(emittedEvents().has("agent-approval-closed")).toBe(true);
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

/**
 * 批准请求里的 `opType` 是第三种没人把关的字符串，而它的坏法最贵：前端不认识就把
 * 整条请求丢掉，后端在那儿白等到超时被拒。界面看起来是"Agent 卡了两分钟"，日志里
 * 只有一行 console.warn —— 而这个仓库最常犯的缺陷正是"加了个新名字却没检查它的消费者"。
 */
describe("approval op types", () => {
    function approvalOpTypes(): string[] {
        const sources = rustSources();
        return [
            ...namesFrom(
                sources,
                stringConstants(sources),
                /ApprovalRequest::new\(\s*"([a-z0-9_]+)"/g,
                /ApprovalRequest::new\(\s*([A-Z][A-Z0-9_]*)\s*,/g
            ),
            ...pointerOpTypes(),
        ];
    }

    /**
     * 指针输入那条路径的 opType 不是字面量：点击、双击、右键、滚轮共用一个函数，动作类型
     * 由 `Gesture::record_kind` 给出，所以上面两条正则一个都匹配不到它。
     *
     * 多一条规则跟着这个间接层走，而不是为了好扫描把代码改回四份复制 —— 但下面的前提断言
     * 才是真正的保险：这条规则失效时，`toContain` 会红，而不是安静地少扫两个名字。
     *
     * 已知残留：这里只认引号里的字面量，不像上面两条那样过一遍 `stringConstants()`。
     * 哪天有人把这些字符串提成常量（`Gesture::Middle => MIDDLE_CLICK_KIND`），新的那个
     * 就会从集合里掉出去而前提断言照样绿 —— 提常量的那个人要顺手把这条规则也补上。
     */

    function pointerOpTypes(): string[] {
        const names = new Set<string>();
        for (const source of rustSources()) {
            const body = source.match(/fn record_kind\(&self\)[\s\S]*?\n    \}/);
            if (!body) {
                continue;
            }
            for (const match of body[0].matchAll(/"([a-z0-9_]+)"/g)) {
                names.add(match[1]);
            }
        }
        return [...names];
    }

    it("the frontend understands every op type the backend asks approval for", () => {
        const ops = approvalOpTypes();

        // 前提检查：解析失效时这条测试会以"全部通过"的方式静默死掉
        expect(ops.length).toBeGreaterThan(0);
        expect(ops).toContain("browser_open");
        expect(ops).toContain("computer_capture");
        expect(ops).toContain("computer_click");
        expect(ops).toContain("computer_scroll");

        const unknown = ops.filter((op) => (normalizeDestructiveOpType(op) === "unknown"));
        expect(
            unknown,
            "the dialog would drop these requests and the run would stall until it times out"
        ).toEqual([]);
    });
});

/**
 * 第四种没人把关的名字：**内置工具**。

 *
 * `SECURITY.md` 是"后端实际强制了什么"的那份文档，而 AGENTS.md 把"文档必须对着代码核过"
 * 写成了规则 —— 靠的是每次有人记得去核。这个会话里我自己就把那条规则破了好几次（数错了
 * 结果的条数、写出代码产生不出的后果）。加一个新工具却忘了写进 SECURITY.md 是同一类漂移，
 * 而且最贵：那份文档正是用来回答"它到底被允许做什么"的。
 *
 * 只查一个方向（每个工具都出现在文档里）。反方向（文档提到一个已经不存在的工具）需要一份
 * 非工具 `workspace_*` 标识符的白名单（`workspace_tool_call`、`<workspace_root>`），那份
 * 白名单本身又会腐烂；而改名会被这个方向抓住 —— 新名字不在文档里。
 */
describe("built-in tool documentation", () => {
    function workspaceToolNames(): string[] {
        const names = new Set<string>();
        for (const source of rustSources()) {
            for (const match of source.matchAll(
                /pub const [A-Z0-9_]+: &str = "(workspace_[a-z_]+)"/g
            )) {
                names.add(match[1]);
            }
        }
        return [...names].sort();
    }

    it("SECURITY.md names every built-in tool the backend can advertise", () => {
        const tools = workspaceToolNames();

        // 前提检查：解析失效时这条测试会以"全部通过"的方式静默死掉
        expect(tools.length, "the scanner found no workspace_* tool constants").toBeGreaterThan(10);
        expect(tools).toContain("workspace_computer_click");
        expect(tools).toContain("workspace_computer_scroll");

        const security = readFileSync("SECURITY.md", "utf8");

        const undocumented = tools.filter((name) => !security.includes(name));
        expect(
            undocumented,
            "these tools exist but SECURITY.md does not name them — the document that answers 'what is it allowed to do' would be wrong"
        ).toEqual([]);
    });
});


