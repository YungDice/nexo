import { useEffect, useRef } from "react";

import { Map as MapLibreMap, setWorkerUrl } from "maplibre-gl";
// Vite emits the worker as an ordinary same-origin asset. That is what keeps
// the CSP untouched: MapLibre otherwise launders its worker through a `blob:`
// URL, and `script-src 'self'` — which `worker-src` falls back to — refuses
// one. Editing the CSP to allow it would be the wrong fix.
import workerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?url";
import "maplibre-gl/dist/maplibre-gl.css";

import type { Pin } from "../../lib/meet";
import { renderChar, type CharConfig } from "./NexoChar";
import { CLUSTER_MAX_ZOOM, MAX_ZOOM, mapStyle } from "./mapStyle";

setWorkerUrl(workerUrl);

/**
 * The map.
 *
 * The rules this file exists to keep, all of which are about staying at sixty
 * frames with ten thousand pins on screen:
 *
 * **Never a DOM marker.** `new maplibregl.Marker()` per person is a `<div>`
 * the browser repositions every frame; it dies in the low hundreds. Everyone
 * is a feature in one clustered `geojson` source, drawn by `symbol` and
 * `circle` layers.
 *
 * **Sprites are made once and capped.** A character is rendered to an image
 * exactly once and kept in an LRU of {@link ATLAS_MAX}. Rendering per frame,
 * or keeping one per person seen, are the two ways this stops being viable.
 *
 * **The context is released on unmount.** WebView2 caps live WebGL contexts,
 * and leaking one per visit to the tab ends with a blank map that looks like a
 * bug in the map rather than in its lifecycle.
 *
 * **Nothing here is live.** Pins arrive as a prop. There is no timer, no
 * subscription and no `syncAgent` involvement — a map that updated itself
 * would turn "where somebody said they are" into "where somebody is now".
 */

/** The most character sprites kept on the GPU at once. */
const ATLAS_MAX = 300;

/** The pixel size a character sprite is rasterised at. */
const SPRITE = 48;

export function MeetMap({
  pins,
  onSelect,
  className,
}: {
  pins: Pin[];
  /** Somebody was tapped. Opens their card. */
  onSelect: (handle: string) => void;
  className?: string;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const map = useRef<MapLibreMap | null>(null);
  const ready = useRef(false);
  // handle -> sprite id, in insertion order, so the oldest is evicted first.
  const atlas = useRef<Map<string, string>>(new Map());
  const latest = useRef<Pin[]>(pins);
  latest.current = pins;

  // The map itself, created once. `pins` is deliberately not a dependency:
  // rebuilding a WebGL context because somebody moved is the opposite of the
  // point, and the effect below pushes data into the existing one instead.
  useEffect(() => {
    if (!host.current) return;

    const created = new MapLibreMap({
      container: host.current,
      style: mapStyle(),
      center: [10, 30],
      zoom: 1.6,
      maxZoom: MAX_ZOOM,
      // Not a limitation, a promise: the map does not go close enough to point
      // at a building, and `MeetAgreement` says so in those words.
      fadeDuration: 0,
      attributionControl: false,
      refreshExpiredTiles: false,
      // `alpha` is absent on purpose. MapLibre builds its context as
      // `{...canvasContextAttributes, alpha: true, ...}` — the spread first,
      // the literal after — so the canvas is transparent whatever is asked
      // for. The opaque surface has to go behind it, which is what the host
      // div's `bg-surface-0` is for.
      canvasContextAttributes: { antialias: false, preserveDrawingBuffer: false },
    });
    map.current = created;

    created.on("load", () => {
      ready.current = true;
      push(created, latest.current, atlas.current);
    });

    // One handler for every person, rather than one per feature.
    created.on("click", "people", (event) => {
      const handle = event.features?.[0]?.properties?.handle;
      if (typeof handle === "string") onSelect(handle);
    });

    // A cluster opens rather than selecting: tapping a crowd means "show me
    // who is in it", and MapLibre can say what zoom breaks it apart.
    created.on("click", "clusters", (event) => {
      const feature = event.features?.[0];
      if (!feature) return;
      created.easeTo({
        center: (feature.geometry as GeoJSON.Point).coordinates as [number, number],
        zoom: Math.min(created.getZoom() + 2, CLUSTER_MAX_ZOOM + 1),
      });
    });

    for (const layer of ["people", "clusters"]) {
      created.on("mouseenter", layer, () => {
        created.getCanvas().style.cursor = "pointer";
      });
      created.on("mouseleave", layer, () => {
        created.getCanvas().style.cursor = "";
      });
    }

    return () => {
      ready.current = false;
      atlas.current.clear();
      // See the header: WebView2 caps live contexts.
      created.remove();
      map.current = null;
    };
  }, [onSelect]);

  // Data, pushed into the existing map.
  useEffect(() => {
    const created = map.current;
    if (!created || !ready.current) return;
    push(created, pins, atlas.current);
  }, [pins]);

  // The theme changed under us. Rebuilding the style keeps the map's colours
  // the app's colours without a second palette existing anywhere.
  useEffect(() => {
    const root = document.documentElement;
    const observer = new MutationObserver(() => {
      const created = map.current;
      if (!created || !ready.current) return;
      created.setStyle(mapStyle());
      // The style swap drops the sprite atlas with it.
      atlas.current.clear();
      created.once("styledata", () => push(created, latest.current, atlas.current));
    });
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);

  return (
    // Opaque, and load-bearing: the GL canvas is transparent and cannot be
    // told otherwise, so without a solid surface underneath it the veiled
    // field — and the desktop behind a DWM backdrop — show through the map.
    <div ref={host} className={`bg-surface-0 ${className ?? ""}`} />
  );
}

/** Feed the source, adding any sprite that is newly on screen. */
function push(map: MapLibreMap, pins: Pin[], atlas: Map<string, string>) {
  const features: GeoJSON.Feature[] = [];

  for (const pin of pins) {
    const id = spriteFor(map, atlas, pin);
    features.push({
      type: "Feature",
      geometry: { type: "Point", coordinates: [pin.lon, pin.lat] },
      properties: { handle: pin.handle, charId: id },
    });
  }

  const source = map.getSource("pins");
  if (source && "setData" in source) {
    (source as { setData: (data: GeoJSON.FeatureCollection) => void }).setData({
      type: "FeatureCollection",
      features,
    });
  }
}

/**
 * The sprite id for somebody, rendering and registering it on first sight.
 *
 * Rendering is the expensive half and happens once per person per style. The
 * LRU is what stops a long session from accumulating an image for everyone
 * ever seen: reaching the cap evicts the least recently added, and the next
 * sight of that person renders them again, which is rare and cheap.
 */
function spriteFor(map: MapLibreMap, atlas: Map<string, string>, pin: Pin): string {
  const existing = atlas.get(pin.handle);
  if (existing) {
    // Touch it, so it is no longer the oldest.
    atlas.delete(pin.handle);
    atlas.set(pin.handle, existing);
    return existing;
  }

  const id = `char-${pin.handle}`;
  atlas.set(pin.handle, id);

  if (atlas.size > ATLAS_MAX) {
    const oldest = atlas.keys().next();
    if (!oldest.done) {
      const evicted = atlas.get(oldest.value);
      atlas.delete(oldest.value);
      if (evicted && map.hasImage(evicted)) map.removeImage(evicted);
    }
  }

  if (!map.hasImage(id)) {
    // A placeholder until the raster lands, so the person is on the map
    // immediately rather than after a decode.
    map.addImage(id, blank());
    void rasterise(pin.char_config as CharConfig).then((bitmap) => {
      if (!bitmap) return;
      // The map may have gone, or the style may have been swapped.
      try {
        if (map.hasImage(id)) map.removeImage(id);
        map.addImage(id, bitmap);
      } catch {
        // A style swap raced the decode. The next push re-adds it.
      }
    });
  }

  return id;
}

/** A transparent square, so a feature can be drawn before its art exists. */
function blank(): ImageData {
  return new ImageData(SPRITE, SPRITE);
}

/**
 * A character, rasterised for the GPU.
 *
 * The SVG is generated in this process and turned into a bitmap through a
 * blob URL, which is a same-origin object rather than a remote document — rule
 * 3 is about what the WebView fetches from elsewhere, and this fetches nothing.
 */
async function rasterise(config: CharConfig): Promise<ImageBitmap | null> {
  try {
    const svg = renderChar(config);
    const blob = new Blob([svg], { type: "image/svg+xml" });
    const url = URL.createObjectURL(blob);
    try {
      const image = new Image(SPRITE, SPRITE);
      await new Promise<void>((resolve, reject) => {
        image.onload = () => resolve();
        image.onerror = () => reject(new Error("character did not decode"));
        image.src = url;
      });
      return await createImageBitmap(image, {
        resizeWidth: SPRITE,
        resizeHeight: SPRITE,
      });
    } finally {
      URL.revokeObjectURL(url);
    }
  } catch {
    // A character that will not draw is not worth a broken map: the feature
    // keeps its blank sprite and everything else carries on.
    return null;
  }
}
