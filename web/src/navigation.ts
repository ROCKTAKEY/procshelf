export type Screen =
  | { kind: "list" }
  | { kind: "process"; id: string; panel: "operations" | "information" };

export function parseScreen(hash: string): Screen {
  const match = /^#\/processes\/([^/]+)\/(operations|information)$/.exec(hash);
  if (!match) return { kind: "list" };
  try {
    const id = decodeURIComponent(match[1]);
    if (!/^[A-Za-z0-9_-]{1,128}$/.test(id)) return { kind: "list" };
    return {
      kind: "process",
      id,
      panel: match[2] === "information" ? "information" : "operations",
    };
  } catch {
    return { kind: "list" };
  }
}
export function screenHash(screen: Screen): string {
  return screen.kind === "list"
    ? "#/"
    : `#/processes/${encodeURIComponent(screen.id)}/${screen.panel}`;
}
export function parentScreen(screen: Screen): Screen {
  return screen.kind === "process" && screen.panel === "information"
    ? { ...screen, panel: "operations" }
    : { kind: "list" };
}
