import * as topojson from "topojson-client";
import world from "world-atlas/countries-110m.json";

/**
 * The basemap.
 *
 * There is no tile server, no API key, no tile request and no attribution
 * obligation, because there are no tiles. `countries-110m` is Natural Earth
 * data in the public domain, bundled as 105 KB of TopoJSON, converted once
 * here and handed to MapLibre as a single GeoJSON source.
 *
 * That is the same sentence the README already uses about icons and avatars:
 * nothing is fetched at runtime. It is also what makes rule 3 hold without
 * argument — a map that loaded tiles would be loading remote documents into
 * the WebView on every pan.
 *
 * The conversion is memoised because it is pure and not free: running it per
 * mount would rebuild every country outline each time the tab is opened.
 */

let cached: GeoJSON.FeatureCollection | null = null;

/** Every country as one GeoJSON collection. Built once per process. */
export function countries(): GeoJSON.FeatureCollection {
  if (cached) return cached;

  const topology = world as unknown as TopoJSON.Topology;
  const object = topology.objects.countries;
  if (!object) {
    // Not a crash: an empty world draws an empty map, and the pins — which are
    // the point — still render over it.
    cached = { type: "FeatureCollection", features: [] };
    return cached;
  }

  cached = topojson.feature(
    topology,
    object,
  ) as unknown as GeoJSON.FeatureCollection;
  return cached;
}
