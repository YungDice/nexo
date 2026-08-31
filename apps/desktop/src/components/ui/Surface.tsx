import type { HTMLAttributes, ReactNode } from "react";
import { cn } from "../../lib/cn";

/**
 * A glass pane.
 *
 * §7.1 in one component: a translucent surface over a blur, with a lit top
 * edge. Components pick a `tone` and never touch `backdrop-filter` — that
 * keeps the opaque fallback and the Settings toggle working everywhere at
 * once (plan risk 9).
 */
export interface PanelProps extends HTMLAttributes<HTMLDivElement> {
  tone?: "rail" | "list" | "content" | "raised";
  edge?: boolean;
}

const tones = {
  rail: "glass-0",
  list: "glass-1",
  content: "glass-2",
  raised: "glass-3",
} as const;

export function Panel({
  tone = "list",
  edge = true,
  className,
  children,
  ...rest
}: PanelProps) {
  return (
    <div className={cn(tones[tone], edge && "edge-top", className)} {...rest}>
      {children}
    </div>
  );
}

/** A section header in the display face, used sparingly so it stays a signal. */
export function SectionHeader({
  children,
  action,
  className,
}: {
  children: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex items-center justify-between gap-3", className)}>
      <h2 className="font-display text-text-hi text-title font-semibold tracking-[-0.01em]">
        {children}
      </h2>
      {action}
    </div>
  );
}

/** The small upper-case label above a group of rows. */
export function GroupLabel({ children }: { children: ReactNode }) {
  return (
    <div className="text-text-lo px-1 text-[11px] font-medium tracking-[0.08em] uppercase">
      {children}
    </div>
  );
}

export function Divider({ className }: { className?: string }) {
  return <div className={cn("h-px bg-[var(--hairline)]", className)} />;
}
