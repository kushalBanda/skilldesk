/** SkillDesk mark (DESIGN.md §2 "Brand"): two stacked cards on a dark tile. */
export function BrandMark({ size = 22 }: { size?: number }) {
  const glyph = Math.round(size * 0.59 * 1.12);
  return (
    <span
      aria-hidden="true"
      className="grid shrink-0 place-items-center"
      style={{
        width: size,
        height: size,
        borderRadius: size * 0.27,
        background: "linear-gradient(#2b2b30, #0b0b0c)",
        boxShadow:
          size >= 40
            ? "inset 0 1px 0 rgba(255,255,255,.16), inset 0 0 0 1px rgba(255,255,255,.06), 0 12px 28px -12px rgba(20,48,72,.55)"
            : "inset 0 1px 0 rgba(255,255,255,.16), inset 0 0 0 1px rgba(255,255,255,.06)",
      }}
    >
      <svg viewBox="0 0 24 24" width={glyph} height={glyph}>
        <rect x="4" y="4" width="12.5" height="7.5" rx="3.75" fill="#ffffff" />
        <rect x="7.5" y="12.5" width="12.5" height="7.5" rx="3.75" fill="#0099ff" />
      </svg>
    </span>
  );
}

/** Tile + wordmark. Sidebar 22px / 18px type; Welcome 44px / 24px type. */
export function BrandLockup({ size = 22 }: { size?: number }) {
  const big = size >= 40;
  return (
    <span
      className="inline-flex items-center font-bold text-ink"
      style={{ gap: size * 0.5, fontSize: big ? 24 : 18, letterSpacing: big ? "-0.035em" : "-0.04em", lineHeight: 1 }}
    >
      <BrandMark size={size} />
      SkillDesk
    </span>
  );
}
