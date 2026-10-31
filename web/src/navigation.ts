export type Screen = { kind: "list" } | { kind: "process"; id: string };

export function parseScreen(hash: string): Screen {
  const match = /^#\/processes\/([^/]+)$/.exec(hash);
  if (!match) return { kind: "list" };
  try {
    const id = decodeURIComponent(match[1]);
    if (!/^[A-Za-z0-9_-]{1,128}$/.test(id)) return { kind: "list" };
    return { kind: "process", id };
  } catch {
    return { kind: "list" };
  }
}
export function screenHash(screen: Screen): string {
  return screen.kind === "list"
    ? "#/"
    : `#/processes/${encodeURIComponent(screen.id)}`;
}
