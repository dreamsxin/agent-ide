import type { ProblemEntry } from "../stores/useProblemStore";

const ANSI_PATTERN = /\x1b\[[0-9;?]*[ -/]*[@-~]/g;
const MAX_BUFFER_LENGTH = 24000;

const LOCATION_PATTERNS = [
  /(?<file>[A-Za-z]:[\\/][^:\r\n]+|\.[\\/][^:\r\n]+|[^:\s\r\n][^:\r\n]*?\.(?:ts|tsx|js|jsx|rs|py|go|vue|svelte|css|scss|html)):(?<line>\d+):(?<column>\d+)(?:\s*[-:]?\s*(?<message>.*))?/g,
  /(?<file>[A-Za-z]:[\\/][^( \r\n]+|\.[\\/][^( \r\n]+|[^(\s\r\n][^(\r\n]*?\.(?:ts|tsx|js|jsx|rs|py|go|vue|svelte|css|scss|html))\((?<line>\d+),(?<column>\d+)\):\s*(?<message>.*)/g,
];

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

  for (const pattern of LOCATION_PATTERNS) {
    pattern.lastIndex = 0;
    for (const match of output.matchAll(pattern)) {
      const groups = match.groups;
      if (!groups) continue;

      const line = Number(groups.line);
      const column = Number(groups.column);
      if (!Number.isFinite(line) || !Number.isFinite(column)) continue;

      const file = normalizeFile(groups.file);
      const message = cleanMessage(groups.message || inferMessage(output, match.index ?? 0));
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
  return file.trim().replace(/\\/g, "/");
}

function cleanMessage(message: string) {
  return message
    .replace(/^error\s+TS\d+:\s*/i, "")
    .replace(/^error:\s*/i, "")
    .replace(/^warning:\s*/i, "")
    .trim();
}

function inferMessage(output: string, index: number) {
  const nextLine = output.slice(index).split("\n")[1] ?? "";
  return nextLine.trim();
}

function inferSeverity(message: string) {
  return /warning|warn/i.test(message) ? "warning" : "error";
}
