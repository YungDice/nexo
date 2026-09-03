import { describe, expect, it } from "vitest";

import { compose, type Draft } from "./compose";

const draft = (over: Partial<Draft> = {}): Draft => ({
  title: "",
  body: "",
  linkUrl: "",
  images: 0,
  ...over,
});

/**
 * What kind of post a draft is.
 *
 * This replaced three tabs, and the reason it can is that the answer was
 * always derivable: whether a post is a link post is entirely answered by
 * whether it has a link in it. What makes the tests worth having is that the
 * derivation has to agree with `apps/server/src/posts.rs` exactly -- the
 * server refuses a link on an image or text post, so an order that put images
 * first would send requests it rejects.
 */
describe("compose", () => {
  it("is text when there is only writing", () => {
    const out = compose(draft({ body: "hello" }));
    expect(out.kind).toBe("text");
    expect(out.linkUrl).toBeNull();
    expect(out.ready).toBe(true);
  });

  it("is an image post when there are images and no link", () => {
    expect(compose(draft({ images: 2 })).kind).toBe("image");
  });

  it("is a link post as soon as there is a link", () => {
    // Even with images: the server allows a link post to carry them, and it
    // refuses to let an image post have a link. Reading images first would
    // produce exactly the request it rejects.
    const out = compose(draft({ linkUrl: "https://example.com", images: 2 }));
    expect(out.kind).toBe("link");
    expect(out.linkUrl).toBe("https://example.com");
  });

  it("sends null rather than an empty link", () => {
    // A text post with link_url: "" is a text post the server refuses.
    expect(compose(draft({ body: "hi", linkUrl: "   " })).linkUrl).toBeNull();
    expect(compose(draft({ body: "hi", linkUrl: "   " })).kind).toBe("text");
  });

  it("says so before sending when a link has no scheme", () => {
    // The server has to check this too -- `javascript:` in a feed is stored
    // XSS -- but somebody who typed example.com should hear it now.
    const out = compose(draft({ linkUrl: "example.com" }));
    expect(out.ready).toBe(false);
    expect(out.problem).toBe("A link has to start with http:// or https://.");
  });

  it("accepts either scheme, in any case", () => {
    for (const url of [
      "http://example.com",
      "https://example.com",
      "HTTPS://EXAMPLE.COM",
    ]) {
      expect(compose(draft({ linkUrl: url })).ready).toBe(true);
    }
  });

  it("is not ready when there is nothing in it", () => {
    expect(compose(draft()).ready).toBe(false);
    expect(compose(draft({ body: "   ", title: "  " })).ready).toBe(false);
  });

  it("is ready on a title alone", () => {
    // The server accepts one, so the button must not be the thing refusing it.
    expect(compose(draft({ title: "Reading list" })).ready).toBe(true);
  });

  it("holds the four-image ceiling", () => {
    expect(compose(draft({ images: 4 })).ready).toBe(true);
    expect(compose(draft({ images: 5 })).ready).toBe(false);
    expect(compose(draft({ images: 5 })).problem).toBe("Up to four images.");
  });
});
