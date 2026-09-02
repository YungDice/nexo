import { describe, expect, it } from "vitest";

import { OptionsDescriptor } from "@dicebear/core";

import { VOXEL_ART, renderChar } from "./NexoChar";

/**
 * The character generator, checked where the product depends on it.
 *
 * Storing a config rather than an image only works if the same config always
 * produces the same picture. If that ever stopped being true, two people
 * looking at the same person would see different characters, and the config
 * would have to become an upload — which is the thing this design exists to
 * avoid.
 */
describe("NexoChar", () => {
  it("renders the same config to the same bytes", () => {
    const config = {
      topVariant: ["hoodie"],
      eyesVariant: ["happy"],
      skinColor: ["ffdfbf"],
    };
    expect(renderChar(config)).toBe(renderChar(config));
  });

  it("renders different configs differently", () => {
    const a = renderChar({ eyesVariant: ["happy"] });
    const b = renderChar({ eyesVariant: ["wide"] });
    expect(a).not.toBe(b);
  });

  it("produces SVG that fetches nothing", () => {
    const svg = renderChar({});
    expect(svg.trimStart().startsWith("<svg")).toBe(true);

    // Rule 3, tested as the rule rather than as a substring. The markup does
    // contain `http://www.w3.org/2000/svg` — that is the XML namespace, which
    // is an identifier and never fetched. What would break rule 3 is markup
    // that goes and gets something: a remote reference, an embedded image, or
    // a script.
    expect(svg).not.toMatch(/<script/i);
    expect(svg).not.toMatch(/<image/i);
    expect(svg).not.toMatch(/(?:xlink:)?href\s*=\s*["']https?:/i);
    expect(svg).not.toMatch(/url\(\s*["']?https?:/i);
    expect(svg).not.toMatch(/src\s*=/i);
  });

  /**
   * The studio builds its controls by walking this. A style that reported
   * nothing would render an empty studio, and the failure would look like a
   * layout bug rather than a missing dependency.
   */
  it("describes the options the studio is built from", () => {
    const descriptor = new OptionsDescriptor(VOXEL_ART).toJSON();
    const keys = Object.keys(descriptor);

    expect(keys.length).toBeGreaterThan(0);

    const variants = keys.filter((k) => k.endsWith("Variant"));
    const colours = keys.filter((k) => k.endsWith("Color"));
    expect(variants.length).toBeGreaterThan(0);
    expect(colours.length).toBeGreaterThan(0);

    // Every enum the studio offers must actually carry values, or it would
    // draw a heading with no choices under it.
    for (const key of variants) {
      const field = descriptor[key];
      if (field && field.type === "enum") {
        expect(field.values.length).toBeGreaterThan(0);
      }
    }
  });

  /**
   * The studio only ever writes values the descriptor reported. This is the
   * property that lets it be generated rather than written: a hardcoded list
   * would drift, and the drift would show up as a character that will not
   * render.
   */
  it("accepts every variant the descriptor reports", () => {
    const descriptor = new OptionsDescriptor(VOXEL_ART).toJSON();
    for (const [key, field] of Object.entries(descriptor)) {
      if (!key.endsWith("Variant") || field.type !== "enum") continue;
      for (const value of field.values) {
        expect(() => renderChar({ [key]: [value] })).not.toThrow();
      }
    }
  });
});
