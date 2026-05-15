import type { ProblemEntry } from "../stores/useProblemStore";
import { normalizeFilePath } from "./paths";

const ANSI_PATTERN = /\x1b\[[0-9;?]*[ -/]*[@-~]/g;
const MAX_BUFFER_LENGTH = 24000;

const LOCATION_PATTERNS = [
  /(?<file>file:\/\/\/?[A-Za-z]:\/[^:\r\n)]+?\.(?:ts|tsx|js|jsx|rs|py|go|vue|svelte|css|scss|html)):(?<line>\d+):(?<column>\d+)(?:\s*[-:]?\s*(?<message>.*))?/g,
  /(?<file>[A-Za-z]:[\\/][^:\r\n]+|\.[\\/][^:\r\n]+|[^:\s\r\n][^:\r\n]*?\.(?:ts|tsx|js|jsx|rs|py|go|vue|svelte|css|scss|html)):(?<line>\d+):(?<column>\d+)(?:\s*[-:]?\s*(?<message>.*))?/g,
  /(?<file>[A-Za-z]:[\\/][^( \r\n]+|\.[\\/][^( \r\n]+|[^(\s\r\n][^(\r\n]*?\.(?:ts|tsx|js|jsx|rs|py|go|vue|svelte|css|scss|html))\((?<line>\d+),(?<column>\d+)\):\s*(?<message>.*)/g,
  /(?:at\s+.*?)?\(?(?<file>file:\/\/\/?[A-Za-z]:\/[^:\r\n)]+|\b[A-Za-z]:[\\/][^:\r\n)]+|\.[\\/][^:\r\n)]+|[^:\s\r\n()][^:\r\n()]*?\.(?:ts|tsx|js|jsx|rs|py|go|vue|svelte|css|scss|html)):(?<line>\d+):(?<column>\d+)\)?/g,
];

const TEST_FILE_PATTERN =
  /(?:^|\n)\s*(?:FAIL|FAILED|✕|×|❯)\s+(?<file>[A-Za-z]:[\\/][^\s\r\n]+|\.[\\/][^\s\r\n]+|[^\s\r\n]+\.(?:test|spec)\.(?:ts|tsx|js|jsx|rs|py|go|vue|svelte))/g;

export interface TerminalProblemParseResult {
  buffer: string;
  problems: ProblemEntry[];
}

export function appendAndParseTerminalProblems(
  previousBuffer: string,
  chunk: string,
  terminalId: string
): TerminalProblemParseResult {
  const buffer = trimBuffer(stripAnsi(previousBuffer + chunk).replace(/\r\n/g, "\n"));
  const problems = parseTerminalProblems(buffer, terminalId);
  return { buffer, problems };
}

export function parseTerminalProblems(output: string, terminalId = "main"): ProblemEntry[] {
  const problems = new Map<string, ProblemEntry>();
  const failedTestFiles = findFailedTestFiles(output);

  for (const pattern of LOCATION_PATTERNS) {
    pattern.lastIndex = 0;
    for (const match of output.matchAll(pattern)) {
      const groups = match.groups;
      if (!groups) continue;

      const line = Number(groups.line);
      const column = Number(groups.column);
      if (!Number.isFinite(line) || !Number.isFinite(column)) continue;

      const file = normalizeFile(groups.file);
      const message = cleanMessage(
        groups.message || inferTestMessage(output, match.index ?? 0) || inferMessage(output, match.index ?? 0)
      );
      const severity = inferSeverity(message);
      const id = `terminal-${terminalId}-${file}-${line}-${column}-${message}`;

      problems.set(id, {
        id,
        file,
        line,
        column,
        severity,
        source: "test",
        message: message || "Terminal reported a problem",
      });
    }
  }

  for (const file of failedTestFiles) {
    const id = `terminal-${terminalId}-${file}-1-1-test-failed`;
    if (problems.has(id) || [...problems.values()].some((problem) => problem.file === file)) {
      continue;
    }
    problems.set(id, {
      id,
      file,
      line: 1,
      column: 1,
      severity: "error",
      source: "test",
      message: "Test failed",
    });
  }

  return [...problems.values()];
}

function stripAnsi(value: string) {
  return value.replace(ANSI_PATTERN, "");
}

function trimBuffer(buffer: string) {
  if (buffer.length <= MAX_BUFFER_LENGTH) return buffer;
  return buffer.slice(buffer.length - MAX_BUFFER_LENGTH);
}

function normalizeFile(file: string) {
  return normalizeFilePath(file.trim());
}

function cleanMessage(message: string) {
  return message
    .replace(/^\s*[❯>]\s*/, "")
    .replace(/^error\s+TS\d+:\s*/i, "")
    .replace(/^error:\s*/i, "")
    .replace(/^warning:\s*/i, "")
    .replace(/^AssertionError:\s*/i, "")
    .trim();
}

function inferMessage(output: string, index: number) {
  const lines = output.slice(index).split("\n").slice(1, 5);
  return lines.map((line) => line.trim()).find(Boolean) ?? "";
}

function inferTestMessage(output: string, index: number) {
  const linesBefore = output.slice(0, index).split("\n").slice(-8).reverse();
  const message = linesBefore.find((line) =>
    /AssertionError|Error:|expected|received|FAIL|FAILED|✕|×/i.test(line)
  );
  return message?.trim() ?? "";
}

function inferSeverity(message: string) {
  return /warning|warn/i.test(message) ? "warning" : "error";
}

function findFailedTestFiles(output: string) {
  const files = new Set<string>();
  TEST_FILE_PATTERN.lastIndex = 0;
  for (const match of output.matchAll(TEST_FILE_PATTERN)) {
    const file = match.groups?.file;
    if (file) files.add(normalizeFile(file));
  }
  return files;
}
