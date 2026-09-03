import { describe, expect, it } from "vitest";
import {
  clockTime,
  dayDivider,
  fileSize,
  initials,
  relativeTime,
  safetyNumber,
  timeLeft,
} from "./format";

const now = new Date(2026, 7, 24, 14, 30);

describe("relativeTime", () => {
  it("counts minutes, then falls back to the clock, then to the calendar", () => {
    expect(relativeTime(new Date(2026, 7, 24, 14, 30), now)).toBe("now");
    expect(relativeTime(new Date(2026, 7, 24, 14, 8), now)).toBe("22m");
    expect(relativeTime(new Date(2026, 7, 24, 9, 45), now)).toBe("9:45 am");
    expect(relativeTime(new Date(2026, 7, 23, 21, 0), now)).toBe("yesterday");
  });

  it("does not call a week-old message recent", () => {
    const old = relativeTime(new Date(2026, 6, 30, 9, 0), now);
    expect(old).not.toMatch(/now|m$/);
  });
});

describe("clockTime", () => {
  it("renders noon and midnight as 12, not 0", () => {
    expect(clockTime(new Date(2026, 7, 24, 12, 5))).toBe("12:05 pm");
    expect(clockTime(new Date(2026, 7, 24, 0, 5))).toBe("12:05 am");
  });
});

describe("dayDivider", () => {
  it("names today and yesterday rather than dating them", () => {
    expect(dayDivider(new Date(2026, 7, 24, 1, 0), now)).toBe("Today");
    expect(dayDivider(new Date(2026, 7, 23, 23, 59), now)).toBe("Yesterday");
    expect(dayDivider(new Date(2026, 7, 1), now)).not.toMatch(/Today|Yesterday/);
  });
});

describe("fileSize", () => {
  it("keeps one decimal below ten and drops it above", () => {
    expect(fileSize(512)).toBe("512 B");
    expect(fileSize(204_800)).toBe("200 kB");
    expect(fileSize(1_268_000)).toBe("1.2 MB");
    expect(fileSize(268_435_456)).toBe("256 MB");
  });
});

describe("safetyNumber", () => {
  it("is twelve groups of five digits (§4.1)", () => {
    const digits = "1".repeat(60);
    const groups = safetyNumber(digits);
    expect(groups).toHaveLength(12);
    expect(groups.every((group) => group.length === 5)).toBe(true);
  });

  it("regroups the spaced form the core returns", () => {
    // `SafetyNumber::to_display_string` is already grouped. Slicing that every
    // five characters would give "0 123" and friends.
    const spaced = Array.from({ length: 12 }, (_, i) => String(i % 10).repeat(5)).join(" ");
    const groups = safetyNumber(spaced);
    expect(groups).toHaveLength(12);
    expect(groups.every((group) => /^\d{5}$/.test(group))).toBe(true);
    expect(groups[1]).toBe("11111");
  });
});

describe("initials", () => {
  it("takes the first and last word, never more than two letters", () => {
    expect(initials("Carter Donin")).toBe("CD");
    expect(initials("Mira")).toBe("M");
    expect(initials("Ada Byron King Lovelace")).toBe("AL");
    expect(initials("   ")).toBe("?");
  });
});

describe("timeLeft", () => {
  it("counts down in whole hours, then whole minutes", () => {
    expect(timeLeft(new Date(2026, 7, 24, 21, 30), now)).toBe("7h left");
    expect(timeLeft(new Date(2026, 7, 24, 14, 52), now)).toBe("22m left");
    expect(timeLeft(new Date(2026, 7, 24, 14, 30, 40), now)).toBe(
      "under a minute left",
    );
  });

  it("rounds down, so the number never over-promises", () => {
    // 59 minutes left is not an hour, and saying so would be a promise the
    // clock does not keep.
    expect(timeLeft(new Date(2026, 7, 24, 15, 29), now)).toBe("59m left");
  });

  it("says gone rather than counting backwards", () => {
    expect(timeLeft(new Date(2026, 7, 24, 14, 30), now)).toBe("gone");
    expect(timeLeft(new Date(2026, 7, 24, 10, 0), now)).toBe("gone");
  });
});
