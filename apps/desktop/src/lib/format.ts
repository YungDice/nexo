/**
 * Formatting helpers shared by every surface.
 *
 * These are pure and take an explicit `now`, which is what makes them testable
 * and what keeps a "5 min ago" from depending on when the test suite runs.
 */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Clock time on a message, e.g. "9:45 am". §7.5 keeps copy lower case. */
export function clockTime(at: Date): string {
  const hours = at.getHours();
  const minutes = at.getMinutes().toString().padStart(2, "0");
  const suffix = hours < 12 ? "am" : "pm";
  const twelve = hours % 12 === 0 ? 12 : hours % 12;
  return `${twelve}:${minutes} ${suffix}`;
}

/**
 * The timestamp in a conversation row or on a post: clock time today, a
 * weekday inside the last week, a date beyond that. Never "just now" for
 * something three days old.
 */
export function relativeTime(at: Date, now: Date): string {
  const elapsed = now.getTime() - at.getTime();
  if (elapsed < MINUTE) return "now";
  if (elapsed < HOUR) return `${Math.floor(elapsed / MINUTE)}m`;
  if (isSameDay(at, now)) return clockTime(at);
  if (isSameDay(at, addDays(now, -1))) return "yesterday";
  if (elapsed < 7 * DAY) return at.toLocaleDateString(undefined, { weekday: "short" });
  return at.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

/**
 * How long something has left, e.g. "7h left".
 *
 * Not `relativeTime` with the arguments the other way round: that one answers
 * "when was this" and reaches for clock times, weekdays and dates, none of
 * which a countdown wants. Everything this is used for lasts under a day, so
 * hours and minutes are the whole vocabulary.
 *
 * Rounds *down*, deliberately. "1h left" on something with 59 minutes to go is
 * a promise the clock will not keep, and a story is the kind of thing people
 * decide what to do about based on the number here.
 */
export function timeLeft(until: Date, now: Date): string {
  const remaining = until.getTime() - now.getTime();
  if (remaining <= 0) return "gone";
  if (remaining < MINUTE) return "under a minute left";
  if (remaining < HOUR) return `${Math.floor(remaining / MINUTE)}m left`;
  return `${Math.floor(remaining / HOUR)}h left`;
}

/** The day divider inside the message scroll. */
export function dayDivider(at: Date, now: Date): string {
  if (isSameDay(at, now)) return "Today";
  if (isSameDay(at, addDays(now, -1))) return "Yesterday";
  return at.toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
  });
}

/** File sizes, rendered in the mono face so columns of them line up. */
export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["kB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

/**
 * §4.1: a safety number is 60 digits, shown as 12 groups of 5. Grouping is
 * the whole point — two people read these aloud to each other.
 */
export function safetyNumber(digits: string): string[] {
  // The Rust side renders these already grouped ("12345 67890 ..."), so strip
  // anything that is not a digit before regrouping. Slicing the spaced form
  // every five characters silently produces groups like "0 123" -- wrong, and
  // wrong in the one place where two people are comparing values out loud.
  const clean = digits.replace(/\D/g, "");
  const groups: string[] = [];
  for (let i = 0; i < clean.length; i += 5) groups.push(clean.slice(i, i + 5));
  return groups;
}

export function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

export function addDays(at: Date, days: number): Date {
  const next = new Date(at);
  next.setDate(next.getDate() + days);
  return next;
}

/** Two initials for the generated avatars. Never more, never an emoji. */
export function initials(displayName: string): string {
  const words = displayName.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  const first = words[0] ?? "";
  const last = words.length > 1 ? (words[words.length - 1] ?? "") : "";
  return (first.slice(0, 1) + last.slice(0, 1)).toUpperCase();
}
