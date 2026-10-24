/**
 * Automated visual smoke check (视觉校对) for the Agent IDE web preview.
 *
 * Launches the Vite dev server (web preview mode — Tauri backend is
 * intentionally unavailable), drives the real UI in a headless browser
 * (uses the locally installed Edge/Chrome via playwright-core, no browser
 * download), verifies key layout elements and interactions, and saves
 * screenshots for human review under test-results/visual/.
 *
 * Usage:
 *   node scripts/visual-check.mjs
 *   VISUAL_BASE_URL=http://localhost:1420 node scripts/visual-check.mjs
 *
 * Exit codes: 0 = all checks passed, 1 = one or more checks failed.
 */

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { chromium } from "playwright-core";

const BASE_URL = process.env.VISUAL_BASE_URL || "http://localhost:1420";
const SPAWN_SERVER = !process.env.VISUAL_BASE_URL;
const SHOT_DIR = resolve("test-results/visual");
const VIEWPORT = { width: 1600, height: 900 };

const REQUIRED_SELECTORS = [
  { selector: '[data-testid="app-root"]', label: "App root layout" },
  { selector: '[data-testid="topbar"]', label: "Top bar" },
  { selector: '[data-testid="left-panel"]', label: "Left explorer panel" },
  { selector: '[data-testid="bottom-panel"]', label: "Bottom panel" },
  { selector: '[data-testid="bottom-tab-terminal"]', label: "Terminal tab" },
  { selector: '[data-testid="bottom-tab-problems"]', label: "Problems tab" },
];

const BOTTOM_TABS = ["terminal", "commands", "problems", "logs"];

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function waitForServer(url, timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(url);
      if (res.ok) return;
    } catch {
      // server not up yet
    }
    await sleep(500);
  }
  throw new Error(`Dev server did not become reachable at ${url} within ${timeoutMs}ms`);
}

function startDevServer() {
  const npmCmd = process.platform === "win32" ? "npm.cmd" : "npm";
  const child = spawn(npmCmd, ["run", "dev"], {
    cwd: resolve(),
    stdio: ["ignore", "pipe", "pipe"],
    shell: process.platform === "win32",
  });
  let output = "";
  child.stdout.on("data", (d) => (output += d.toString()));
  child.stderr.on("data", (d) => (output += d.toString()));
  return {
    child,
    getOutput: () => output,
    kill: () => {
      try {
        child.kill();
        if (process.platform === "win32") {
          spawn("taskkill", ["/pid", String(child.pid), "/f", "/t"]);
        }
      } catch {
        // already gone
      }
    },
  };
}

async function launchBrowser() {
  // Prefer the locally installed Edge/Chrome — playwright-core ships no
  // browsers. The sandboxless fallback covers CI runners where kernel user
  // namespaces are restricted; locally it is simply never reached.
  const attempts = [
    { channel: "msedge" },
    { channel: "chrome" },
    { channel: "chrome", args: ["--no-sandbox", "--disable-setuid-sandbox"] },
    { args: ["--no-sandbox", "--disable-setuid-sandbox"] },
  ];
  for (const options of attempts) {
    try {
      return await chromium.launch({ headless: true, ...options });
    } catch (error) {
      lastBrowserError = error;
    }
  }
  throw new Error(
    `Could not launch Edge or Chrome. Install Microsoft Edge or Google Chrome. Last error: ${lastBrowserError}`
  );
}

let lastBrowserError = null;

async function main() {
  mkdirSync(SHOT_DIR, { recursive: true });
  const results = [];
  const pageErrors = [];

  const server = SPAWN_SERVER ? startDevServer() : null;
  let browser;

  try {
    if (server) {
      console.log("Starting Vite dev server...");
      await waitForServer(BASE_URL);
      console.log("Dev server is up.");
    }

    console.log("Launching browser...");
    browser = await launchBrowser();
    console.log("Browser launched.");
    const page = await browser.newPage({ viewport: VIEWPORT });
    page.on("pageerror", (error) => pageErrors.push(String(error)));

    const check = (name, passed, detail = "") => {
      results.push({ name, passed, detail });
      console.log(`${passed ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
    };

    // 1. App boots in web preview mode.
    await page.goto(BASE_URL, { waitUntil: "load", timeout: 60_000 });
    await page.waitForSelector('[data-testid="app-root"]', { timeout: 30_000 });
    await sleep(1500); // let lazy panels settle

    // 2. Required layout elements are present.
    for (const { selector, label } of REQUIRED_SELECTORS) {
      const visible = await page.isVisible(selector);
      check(`${label} visible (${selector})`, visible);
    }

    await page.screenshot({ path: resolve(SHOT_DIR, "01-default-layout.png") });

    // 3. Bottom panel tab switching keeps the panel mounted and renders content.
    for (const tab of BOTTOM_TABS) {
      const tabSelector = `[data-testid="bottom-tab-${tab}"]`;
      if (!(await page.isVisible(tabSelector))) {
        check(`Bottom tab "${tab}" clickable`, false, "tab not found");
        continue;
      }
      await page.click(tabSelector);
      await sleep(400);
      const panelVisible = await page.isVisible('[data-testid="bottom-panel"]');
      check(`Bottom tab "${tab}" renders inside bottom panel`, panelVisible);
      await page.screenshot({ path: resolve(SHOT_DIR, `02-bottom-${tab}.png`) });
    }

    // 4. Command palette opens and closes (Ctrl+Shift+P). The UI is
    // localized, so accept either the English or the Chinese placeholder.
    await page.keyboard.press("Control+Shift+KeyP");
    await sleep(400);
    const paletteShown = await page
      .locator('input[placeholder*="Search commands"]')
      .or(page.locator('input[placeholder*="搜命令"]'))
      .first()
      .isVisible()
      .catch(() => false);
    check("Command palette opens with Ctrl+Shift+P", paletteShown);
    await page.screenshot({ path: resolve(SHOT_DIR, "03-command-palette.png") });
    await page.keyboard.press("Escape");
    await sleep(200);

    // 5. Chat context preview exposes the per-run context toggles, including
    // the AGENTS.md project-memory toggle.
    const contextEdit = page
      .locator('button[title="See and choose what the Agent is given as context"]')
      .or(page.locator('button[title="看一眼并挑选发给 Agent 的上下文"]'))
      .first();
    if (await contextEdit.isVisible().catch(() => false)) {
      await contextEdit.click();
      await sleep(300);
      const agentsToggle = await page
        .locator('label:has-text("AGENTS.md"), div:has-text("AGENTS.md")')
        .first()
        .isVisible()
        .catch(() => false);
      check("Chat context preview shows AGENTS.md toggle", agentsToggle);
      await page.screenshot({ path: resolve(SHOT_DIR, "04-context-preview.png") });
    } else {
      check("Chat context preview shows AGENTS.md toggle", false, "context preview button not found");
    }

    // 6. Agent settings exposes the permission model (Phase 9.0.4).
    // Presets are localized: read-only / create-files / run-commands.
    const agentSettingsTab = page.locator('[data-testid="agent-tab-settings"]');
    if (await agentSettingsTab.isVisible().catch(() => false)) {
      await agentSettingsTab.click();
      await sleep(500);
      const permissionsHeader = page
        .locator("text=What the Agent may do")
        .or(page.locator("text=Agent 能做什么"))
        .first();
      const headerVisible = await permissionsHeader.isVisible().catch(() => false);
      if (headerVisible) {
        await permissionsHeader.scrollIntoViewIfNeeded();
        await sleep(200);
      }
      const presetButtons = await page
        .locator(
          'button:has-text("Read only"), button:has-text("只读"), button:has-text("Create files"), button:has-text("可新建文件"), button:has-text("Run commands"), button:has-text("可跑命令")'
        )
        .count();
      check(
        "Agent settings shows permission presets",
        headerVisible && presetButtons >= 3,
        `header=${headerVisible}, presetButtons=${presetButtons}`
      );
      await page.screenshot({ path: resolve(SHOT_DIR, "05-agent-permissions.png") });
    } else {
      check("Agent settings shows permission presets", false, "agent settings tab not found");
    }

    // 7. No uncaught page errors during the run.
    check(
      "No uncaught page errors",
      pageErrors.length === 0,
      pageErrors.slice(0, 3).join(" | ")
    );

    writeFileSync(
      resolve(SHOT_DIR, "summary.json"),
      JSON.stringify(
        { baseUrl: BASE_URL, viewport: VIEWPORT, results, pageErrors, screenshots: SHOT_DIR },
        null,
        2
      )
    );

    const failed = results.filter((r) => !r.passed);
    console.log(
      `\nVisual check finished: ${results.length - failed.length}/${results.length} checks passed. Screenshots in ${SHOT_DIR}`
    );
    if (failed.length > 0) {
      process.exitCode = 1;
    }
  } finally {
    await browser?.close();
    server?.kill();
  }
}

main().catch((error) => {
  console.error("Visual check crashed:", error);
  process.exitCode = 1;
});
