import { describe, expect, it } from "vitest";

import { attachmentKind, isPlayable } from "./media";

/**
 * Which player a bubble reaches for.
 *
 * The rule that needs writing down is the one about sound: WAV and FLAC are
 * shown as voice messages and everything else with sound in it gets an ordinary
 * player. That is a reading rather than a fact -- there is no recorder in the
 * app yet to mark anything -- so it is worth being explicit about, including
 * where it is deliberately generous (the several names WAV goes by) and where
 * it deliberately gives up.
 */
describe("attachmentKind", () => {
  it("reads pictures and video from the type", () => {
    expect(attachmentKind("image/png")).toBe("image");
    expect(attachmentKind("image/webp")).toBe("image");
    expect(attachmentKind("video/mp4")).toBe("video");
    expect(attachmentKind("video/quicktime")).toBe("video");
  });

  it("treats the uncompressed formats as voice", () => {
    // Every name WAV is sent under. One of them missing means a voice message
    // arriving as a track, which is only a wrong shape -- but it is the shape
    // the whole distinction exists to get right.
    for (const mime of [
      "audio/wav",
      "audio/x-wav",
      "audio/wave",
      "audio/vnd.wave",
      "audio/flac",
      "audio/x-flac",
    ]) {
      expect(attachmentKind(mime)).toBe("voice");
    }
  });

  it("believes a sender who says they recorded it", () => {
    // The whole reason the flag exists. A recorder writes WebM/Opus, which by
    // MIME type alone is indistinguishable from a video clip somebody attached
    // -- so the guess below gets it wrong, and the flag has to win.
    expect(attachmentKind("audio/webm")).toBe("audio");
    expect(attachmentKind("audio/webm", true)).toBe("voice");
    expect(attachmentKind("video/webm", true)).toBe("voice");
  });

  it("still guesses for messages sent before the flag existed", () => {
    // A v0.1.20 payload carries no `voice`, so the extension list is all there
    // is. It stays the fallback rather than being dropped.
    expect(attachmentKind("audio/wav", false)).toBe("voice");
    expect(attachmentKind("audio/mpeg", false)).toBe("audio");
  });

  it("treats the rest of sound as a track", () => {
    expect(attachmentKind("audio/mpeg")).toBe("audio");
    expect(attachmentKind("audio/mp4")).toBe("audio");
    expect(attachmentKind("audio/ogg")).toBe("audio");
    expect(attachmentKind("audio/aac")).toBe("audio");
  });

  it("ignores the case a sender chose", () => {
    expect(attachmentKind("AUDIO/WAV")).toBe("voice");
    expect(attachmentKind("Image/PNG")).toBe("image");
  });

  it("gives up rather than guessing", () => {
    // A file row saves and opens elsewhere, so it works for anything. A player
    // that cannot play is a dead control.
    expect(attachmentKind("application/pdf")).toBe("file");
    expect(attachmentKind("application/octet-stream")).toBe("file");
    expect(attachmentKind("")).toBe("file");
    expect(attachmentKind("text/plain")).toBe("file");
  });

  it("marks everything but a file as playable", () => {
    expect(isPlayable("image")).toBe(true);
    expect(isPlayable("video")).toBe(true);
    expect(isPlayable("audio")).toBe(true);
    expect(isPlayable("voice")).toBe(true);
    expect(isPlayable("file")).toBe(false);
  });
});
