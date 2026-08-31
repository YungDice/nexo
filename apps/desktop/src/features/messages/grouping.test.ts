import { describe, expect, it } from "vitest";
import type { Message } from "../../lib/types";
import { buildRows } from "./MessageList";

const now = new Date(2026, 7, 24, 14, 30);

function message(id: string, authorId: string, at: Date): Message {
  return { id, conversationId: "c", authorId, body: id, at, state: "read" };
}

describe("buildRows", () => {
  it("groups consecutive messages from one person inside five minutes", () => {
    const rows = buildRows(
      [
        message("a", "u-1", new Date(2026, 7, 24, 10, 0)),
        message("b", "u-1", new Date(2026, 7, 24, 10, 2)),
        message("c", "u-1", new Date(2026, 7, 24, 10, 9)),
      ],
      now,
    );

    expect(rows.map((row) => row.startsRun)).toEqual([true, false, true]);
    // The avatar and the timestamp go on the last message of a run.
    expect(rows.map((row) => row.endsRun)).toEqual([false, true, true]);
  });

  it("breaks a run when the sender changes", () => {
    const rows = buildRows(
      [
        message("a", "u-1", new Date(2026, 7, 24, 10, 0)),
        message("b", "u-2", new Date(2026, 7, 24, 10, 1)),
      ],
      now,
    );
    expect(rows[1]?.startsRun).toBe(true);
    expect(rows[0]?.endsRun).toBe(true);
  });

  it("puts a divider on the first message of each day and nowhere else", () => {
    const rows = buildRows(
      [
        message("a", "u-1", new Date(2026, 7, 23, 22, 0)),
        message("b", "u-1", new Date(2026, 7, 24, 9, 0)),
        message("c", "u-1", new Date(2026, 7, 24, 9, 1)),
      ],
      now,
    );
    expect(rows.map((row) => row.divider)).toEqual([
      expect.any(String),
      "Today",
      undefined,
    ]);
    // A new day always starts a new run, even from the same person.
    expect(rows[1]?.startsRun).toBe(true);
  });
});
