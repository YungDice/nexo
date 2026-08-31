/** Join class names, dropping anything falsy. Small enough not to be a
    dependency. */
export function cn(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}
