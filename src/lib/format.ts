const nf = new Intl.NumberFormat("en-US");
const df = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric" });

export function formatCount(n: number): string {
  return nf.format(n);
}

/** "Nov 8, 2026" — unambiguous across locales. */
export function formatDate(iso: string | null | undefined): string {
  return iso ? df.format(new Date(iso)) : "";
}

/** Home folder shown as `~`. */
export function tildePath(p: string): string {
  return p.replace(/^\/Users\/[^/]+/, "~");
}

/** 95.1 min · 20.2 s · 35 ms. */
export function formatMs(ms: number): string {
  if (ms >= 60_000) return `${(ms / 60_000).toFixed(1)} min`;
  if (ms >= 1_000) return `${(ms / 1_000).toFixed(1)} s`;
  return `${Math.round(ms)} ms`;
}

/** 56 MB · 4.1 MB · 812 KB. */
export function formatBytes(n: number): string {
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
  if (n >= 10 * 1024 ** 2) return `${Math.round(n / 1024 ** 2)} MB`;
  if (n >= 1024 ** 2) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

/** Markdown body without its YAML frontmatter. */
export function stripFrontmatter(md: string): string {
  return md.replace(/^\ufeff?---\n[\s\S]*?\n---\n?/, "");
}
