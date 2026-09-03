import { describe, expect, it } from "vitest";

import { CENTRED, clampPan, clampZoom, panBounds, zoomAbout } from "./pan";

describe("panBounds", () => {
  it("is half of what hangs off each side", () => {
    // 1000 wide in an 800 frame: 100 off each side, so 100 either way.
    expect(panBounds({ x: 1000, y: 600 }, { x: 800, y: 400 })).toEqual({
      x: 100,
      y: 100,
    });
  });

  it("is nothing when the picture already fits", () => {
    // Dragging a picture that fits is what makes a viewer feel loose.
    expect(panBounds({ x: 400, y: 300 }, { x: 800, y: 600 })).toEqual(CENTRED);
  });

  it("is nothing on the axis that fits, even when the other does not", () => {
    // A panorama: wide enough to drag sideways, short enough to stay put.
    expect(panBounds({ x: 2000, y: 300 }, { x: 800, y: 600 })).toEqual({
      x: 600,
      y: 0,
    });
  });
});

describe("clampPan", () => {
  const scaled = { x: 1000, y: 600 };
  const frame = { x: 800, y: 400 };

  it("leaves a pan inside the bounds alone", () => {
    expect(clampPan({ x: 40, y: -30 }, scaled, frame)).toEqual({ x: 40, y: -30 });
  });

  it("stops the picture leaving the frame", () => {
    expect(clampPan({ x: 5000, y: -5000 }, scaled, frame)).toEqual({
      x: 100,
      y: -100,
    });
  });

  it("pulls a picture that fits back to the middle", () => {
    // Zooming back out with a pan still applied would otherwise leave the
    // picture stuck off-centre with no way to drag it back.
    expect(clampPan({ x: 90, y: 90 }, { x: 200, y: 200 }, frame)).toEqual(CENTRED);
  });
});

describe("zoomAbout", () => {
  it("keeps the point under the cursor under the cursor", () => {
    // Point 200px right of centre, zoom 1 -> 2. That point of the picture is
    // 200 units out; at 2x it would be drawn 400 out, so the pan must pull
    // back 200 to leave it where the cursor is.
    expect(zoomAbout({ x: 200, y: 0 }, CENTRED, 1, 2)).toEqual({ x: -200, y: 0 });
  });

  it("does nothing when the zoom does not change", () => {
    expect(zoomAbout({ x: 130, y: -70 }, { x: 12, y: 8 }, 2, 2)).toEqual({
      x: 12,
      y: 8,
    });
  });

  it("returns to no pan when zooming back out about the same point", () => {
    // The round trip has to close, or repeated wheeling walks the picture off
    // the screen a little at a time.
    const zoomed = zoomAbout({ x: 200, y: 90 }, CENTRED, 1, 3);
    expect(zoomAbout({ x: 200, y: 90 }, zoomed, 3, 1)).toEqual(CENTRED);
  });

  it("leaves the centre alone whatever the zoom", () => {
    expect(zoomAbout(CENTRED, CENTRED, 1, 4)).toEqual(CENTRED);
  });
});

describe("clampZoom", () => {
  it("holds the range", () => {
    expect(clampZoom(9, 1, 4)).toBe(4);
    expect(clampZoom(0.2, 1, 4)).toBe(1);
  });

  it("rounds off what a wheel leaves behind", () => {
    // A wheel emits fractional deltas; unrounded, the readout says 99% at rest.
    expect(clampZoom(1.0000001, 1, 4)).toBe(1);
    expect(clampZoom(2.33333333, 1, 4)).toBe(2.333);
  });
});
