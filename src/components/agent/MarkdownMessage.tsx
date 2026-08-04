import ReactMarkdown from "react-markdown";

export default function MarkdownMessage({ content }: { content: string }) {
  return <ReactMarkdown skipHtml>{sanitizeMarkdown(content)}</ReactMarkdown>;
}

function sanitizeMarkdown(raw: string) {
  const lines = raw.split("\n");
  let inBlock = false;

  const result = lines.map((line) => {
    if (line.trim().startsWith("```")) inBlock = !inBlock;
    return line;
  });

  if (inBlock) result.push("```");
  return result.join("\n");
}
