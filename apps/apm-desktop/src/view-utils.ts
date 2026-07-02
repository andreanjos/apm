export function escapeHtml(value: string) {
  return value.replace(/[&<>"']/g, (char) => {
    const entities: Record<string, string> = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    };
    return entities[char] ?? char;
  });
}

export function fileNameFromPath(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export function formatBytes(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const mib = bytes / 1024 / 1024;
  if (mib >= 1) {
    return `${mib.toFixed(1)} MB`;
  }
  return `${(bytes / 1024).toFixed(1)} KB`;
}

export function formatLabel(format: string) {
  const normalized = format.toLowerCase();
  if (normalized === "au") {
    return "AU";
  }
  if (normalized === "vst3") {
    return "VST3";
  }
  if (normalized === "app") {
    return "APP";
  }
  return format;
}
