import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cn } from "../../lib/cn";
import { Icon, type IconName } from "./Icon";

type Variant = "primary" | "secondary" | "ghost" | "danger";

const base =
  "inline-flex items-center justify-center gap-2 rounded-control font-medium " +
  "transition-[background-color,color,box-shadow,transform] duration-[var(--motion-fast)] " +
  "ease-[var(--ease-state)] active:translate-y-px disabled:pointer-events-none disabled:opacity-45";

const variants: Record<Variant, string> = {
  // §7.1: the accent is the outgoing bubble and the primary action. It is used
  // sparingly enough that it still means "this one".
  primary:
    "bg-accent text-white hover:bg-accent-soft shadow-[0_6px_20px_-8px_rgba(123,92,250,0.9)]",
  secondary:
    "border border-line-strong bg-fill text-text-hi hover:bg-fill-active",
  ghost: "text-text-mid hover:bg-fill-hover hover:text-text-hi",
  danger: "border border-danger/40 text-danger hover:bg-danger/12",
};

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  icon?: IconName;
  children?: ReactNode;
}

export function Button({
  variant = "secondary",
  icon,
  className,
  children,
  ...rest
}: ButtonProps) {
  return (
    <button
      type="button"
      className={cn(base, variants[variant], "h-9 px-3.5 text-body", className)}
      {...rest}
    >
      {icon ? <Icon name={icon} size={16} /> : null}
      {children}
    </button>
  );
}

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  name: IconName;
  /**
   * Required, not optional. §7.4 puts screen-reader labels on icon-only
   * buttons in the quality floor, and a required prop is the only way that
   * survives contact with a deadline.
   */
  label: string;
  size?: number;
  variant?: Variant;
  active?: boolean;
}

export function IconButton({
  name,
  label,
  size = 18,
  variant = "ghost",
  active = false,
  className,
  ...rest
}: IconButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active || undefined}
      className={cn(
        base,
        variants[variant],
        "size-9 shrink-0",
        active && "bg-accent/16 text-accent-soft",
        className,
      )}
      {...rest}
    >
      <Icon name={name} size={size} />
    </button>
  );
}
