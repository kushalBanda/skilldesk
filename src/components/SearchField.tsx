// Search box (shadcn InputGroup pattern on SkillDesk tokens): icon addon,
// input, then an inline-end addon for a result count, a clear button and the
// ⌘F hint. The whole group carries the focus ring, not the bare input.
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { t } from "../strings";

export const SearchField = forwardRef<
  HTMLInputElement,
  {
    value: string;
    onChange: (v: string) => void;
    label: string;
    placeholder?: string;
    /** Shown at the end while there is a query ("12 results"). */
    count?: string | null;
    /** ⌘F focuses this field (one per screen). */
    hotkey?: boolean;
    busy?: boolean;
    className?: string;
  }
>(function SearchField({ value, onChange, label, placeholder, count, hotkey = true, busy, className = "" }, ref) {
  const input = useRef<HTMLInputElement>(null);
  useImperativeHandle(ref, () => input.current!);
  useEffect(() => {
    if (!hotkey) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "f" && !document.querySelector("[aria-modal='true']")) {
        e.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [hotkey]);
  return (
    <div
      role="search"
      className={[
        "group/search relative flex h-9 items-center rounded-full bg-sunk ring-1 ring-transparent transition-[box-shadow,background-color] duration-150",
        "hover:ring-rule focus-within:bg-bg focus-within:ring-accent focus-within:shadow-[0_0_0_4px_var(--accent-soft)]",
        className,
      ].join(" ")}
    >
      <span className="pointer-events-none flex h-full items-center pl-3 pr-2 text-ink-3 group-focus-within/search:text-ink-2" aria-hidden="true">
        {busy ? (
          <svg width="15" height="15" viewBox="0 0 16 16" className="animate-spin" fill="none">
            <circle cx="8" cy="8" r="6" stroke="currentColor" strokeOpacity=".25" strokeWidth="1.6" />
            <path d="M14 8a6 6 0 00-6-6" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
          </svg>
        ) : (
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
            <circle cx="7" cy="7" r="4.5" />
            <path d="M10.5 10.5L14 14" />
          </svg>
        )}
      </span>
      <input
        ref={input}
        type="text"
        inputMode="search"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value) {
            e.preventDefault();
            e.stopPropagation();
            onChange("");
          }
        }}
        placeholder={placeholder ?? label}
        aria-label={label}
        spellCheck={false}
        autoCorrect="off"
        autoCapitalize="off"
        className="h-full min-w-0 flex-1 bg-transparent text-[13px] text-ink caret-accent outline-none placeholder:text-ink-3 focus-visible:outline-none"
      />
      <span className="flex h-full items-center gap-1.5 pl-2 pr-1.5">
        {value && count ? <span className="tnum whitespace-nowrap text-[12px] text-ink-3">{count}</span> : null}
        {value ? (
          <button
            type="button"
            onClick={() => {
              onChange("");
              input.current?.focus();
            }}
            aria-label={t.clearSearch}
            title={t.clearSearch}
            className="flex h-6 w-6 items-center justify-center rounded-full text-ink-3 hover:bg-sunk hover:text-ink active:scale-90"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
              <path d="M3 3l6 6M9 3l-6 6" />
            </svg>
          </button>
        ) : hotkey ? (
          <kbd className="mr-1 rounded-[6px] px-1.5 font-sans text-[11px] leading-[18px] text-ink-3 ring-1 ring-rule">⌘F</kbd>
        ) : null}
      </span>
    </div>
  );
});
