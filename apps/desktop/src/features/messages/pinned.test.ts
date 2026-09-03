import { describe, expect, it } from "vitest";

import type { Attachment, Message } from "../../lib/types";
import { pinnedLine } from "./pinned";

const message = (over: Partial<Message>): Message => ({
  id: "1",
  conversationId: "c1",
  authorId: "me",
  body: "",
  at: new Date(0),
  state: "sent",
  ...over,
});

const file = (over: Partial<Attachment>): Attachment => ({
  id: "1",
  name: "IMG_4021.jpg",
  size: 100,
  mime: "image/jpeg",
  kind: "image",
  ...over,
});

/**
 * What the pinned list says about a message.
 *
 * Any kind of message can be pinned, so any kind has to be listable. The panel
 * used to render `body || "(no text)"`, which meant pinning a photograph put
 * the words "(no text)" in the panel and lost the photograph -- a pin that
 * shows nothing of what was pinned did not work, as far as the person who made
 * it is concerned.
 */
describe("pinnedLine", () => {
  it("is the words, when there are words", () => {
    expect(pinnedLine(message({ body: "the address is 14b" }))).toEqual({
      text: "the address is 14b",
      described: false,
    });
  });

  it("names the file when there are no words", () => {
    expect(pinnedLine(message({ attachments: [file({})] }))).toEqual({
      icon: "image",
      text: "IMG_4021.jpg",
      described: true,
    });
  });

  it("prefers a caption to the file name that carries it", () => {
    // If somebody wrote something they wrote it for a reason, and IMG_4021.jpg
    // is not information. The icon stays, so the line still says there is a
    // picture attached to those words.
    expect(
      pinnedLine(message({ body: "here it is", attachments: [file({})] })),
    ).toEqual({ icon: "image", text: "here it is", described: false });
  });

  it("does not show what a recorder called a voice message", () => {
    expect(
      pinnedLine(
        message({
          attachments: [file({ name: "rec_0007.wav", kind: "voice" })],
        }),
      ),
    ).toEqual({ icon: "mic", text: "Voice message", described: true });
  });

  it("gives each kind its own mark", () => {
    const iconFor = (kind: Attachment["kind"]) =>
      pinnedLine(message({ attachments: [file({ kind })] })).icon;
    expect(iconFor("image")).toBe("image");
    expect(iconFor("video")).toBe("camera");
    expect(iconFor("audio")).toBe("music");
    expect(iconFor("voice")).toBe("mic");
    expect(iconFor("file")).toBe("file");
  });

  it("says what happened to a message that is no longer there", () => {
    // A pin outlives the message being taken back. The line has to be honest
    // about that rather than showing the words that were withdrawn.
    expect(
      pinnedLine(message({ body: "never mind", retracted: true })),
    ).toEqual({ icon: "close", text: "Taken back", described: true });
    expect(pinnedLine(message({ undecryptable: true })).text).toBe(
      "Could not be opened",
    );
    // `unsupported` carries the payload kind this build did not know, not a
    // flag -- so the check is on it being there, not on it being true.
    expect(pinnedLine(message({ unsupported: "poll" })).text).toBe(
      "Needs a newer version",
    );
  });

  it("never returns an empty line", () => {
    expect(pinnedLine(message({})).text).toBe("Empty message");
  });
});
