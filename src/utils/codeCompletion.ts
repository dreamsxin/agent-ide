export type CompletionCandidateKind = "keyword" | "symbol" | "file" | "snippet";

export interface CompletionCandidate {
  label: string;
  insertText: string;
  detail: string;
  kind: CompletionCandidateKind;
  score: number;
}

interface BuildCompletionCandidatesInput {
  content: string;
  language: string;
  currentWord: string;
  linePrefix: string;
  openFilePaths: string[];
}

const LANGUAGE_KEYWORDS: Record<string, string[]> = {
  javascript: [
    "async",
    "await",
    "const",
    "export",
    "function",
    "import",
    "interface",
    "let",
    "return",
    "type",
  ],
  typescript: [
    "async",
    "await",
    "const",
    "export",
    "function",
    "import",
    "interface",
    "let",
    "return",
    "type",
  ],
  rust: [
    "async",
    "await",
    "enum",
    "fn",
    "impl",
    "let",
    "match",
    "mod",
    "pub",
    "struct",
    "trait",
    "use",
  ],
  python: [
    "async",
    "await",
    "class",
    "def",
    "from",
    "import",
    "lambda",
    "return",
    "self",
    "with",
  ],
  css: ["align-items", "background", "border", "color", "display", "flex", "grid", "margin", "padding"],
  html: ["article", "button", "div", "footer", "header", "input", "main", "section", "span"],
};

const SNIPPETS: Record<string, Array<Omit<CompletionCandidate, "score">>> = {
  javascript: [
    {
      label: "console.log",
      insertText: "console.log($1);",
      detail: "snippet",
      kind: "snippet",
    },
  ],
  typescript: [
    {
      label: "console.log",
      insertText: "console.log($1);",
      detail: "snippet",
      kind: "snippet",
    },
  ],
  rust: [
    {
      label: "println!",
      insertText: "println!(\"{}\", $1);",
      detail: "snippet",
      kind: "snippet",
    },
  ],
};

const IDENTIFIER_PATTERN = /[A-Za-z_$][A-Za-z0-9_$]{2,}/g;

export function buildLocalCompletionCandidates({
  content,
  language,
  currentWord,
  linePrefix,
  openFilePaths,
}: BuildCompletionCandidatesInput): CompletionCandidate[] {
  const normalizedWord = currentWord.toLowerCase();
  const pathPrefix = extractPathPrefix(linePrefix);
  const candidates = new Map<string, CompletionCandidate>();

  for (const keyword of LANGUAGE_KEYWORDS[language] ?? []) {
    addCandidate(candidates, {
      label: keyword,
      insertText: keyword,
      detail: `${language} keyword`,
      kind: "keyword",
      score: 300,
    });
  }

  for (const snippet of SNIPPETS[language] ?? []) {
    addCandidate(candidates, {
      ...snippet,
      score: 250,
    });
  }

  const symbolCounts = countIdentifiers(content);
  for (const [symbol, count] of symbolCounts) {
    addCandidate(candidates, {
      label: symbol,
      insertText: symbol,
      detail: "workspace symbol",
      kind: "symbol",
      score: 100 + Math.min(count, 50),
    });
  }

  if (pathPrefix) {
    for (const filePath of openFilePaths) {
      const name = filePath.split(/[\\/]/).pop() || filePath;
      addCandidate(candidates, {
        label: name,
        insertText: filePath.replace(/\\/g, "/"),
        detail: "open file path",
        kind: "file",
        score: 220,
      });
    }
  }

  return [...candidates.values()]
    .filter((candidate) => {
      if (candidate.kind === "file" && pathPrefix) {
        return candidate.insertText.toLowerCase().includes(pathPrefix.toLowerCase());
      }
      if (!normalizedWord) return candidate.kind !== "file";
      return candidate.label.toLowerCase().startsWith(normalizedWord);
    })
    .filter((candidate) => candidate.label !== currentWord)
    .sort((a, b) => b.score - a.score || a.label.localeCompare(b.label))
    .slice(0, 80);
}

function countIdentifiers(content: string) {
  const counts = new Map<string, number>();
  for (const match of content.matchAll(IDENTIFIER_PATTERN)) {
    const symbol = match[0];
    counts.set(symbol, (counts.get(symbol) ?? 0) + 1);
  }
  return counts;
}

function extractPathPrefix(linePrefix: string) {
  const match = linePrefix.match(/["'`]([^"'`]*)$/);
  if (!match) return null;
  const value = match[1];
  if (!value.includes("/") && !value.includes("\\") && !value.startsWith(".")) return null;
  return value.replace(/\\/g, "/").toLowerCase();
}

function addCandidate(candidates: Map<string, CompletionCandidate>, candidate: CompletionCandidate) {
  const existing = candidates.get(candidate.label);
  if (!existing || candidate.score > existing.score) {
    candidates.set(candidate.label, candidate);
  }
}
