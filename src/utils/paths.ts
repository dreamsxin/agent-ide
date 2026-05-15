export function normalizeFilePath(path: string) {
  return decodePath(path)
    .replace(/^file:\/\/\/?/i, "")
    .replace(/^\/([a-zA-Z]:)/, "$1")
    .replace(/\\/g, "/");
}

export function pathKey(path: string) {
  return normalizeFilePath(path).toLowerCase();
}

export function fileNameFromPath(path: string) {
  const normalized = normalizeFilePath(path);
  return normalized.split("/").pop() || normalized;
}

export function pathsEqual(a: string, b: string) {
  return pathKey(a) === pathKey(b);
}

function decodePath(path: string) {
  try {
    return decodeURIComponent(path);
  } catch {
    return path;
  }
}
