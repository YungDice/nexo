import type { StyleSpecification } from "maplibre-gl";

import { countries } from "./world";

/**
 * The map's own style, built from design tokens.
 *
 * **A hex code in this file is a bug**, exactly as it is in a `.tsx`. Water,
 * land, borders and the pin colours all resolve from CSS custom properties at
 * the moment the map is created, and are rebuilt when the theme changes — so
 * the map is light in a light theme without a second palette existing anywhere.
 *
 * `token()` reads the computed value rather than importing `tokens.json`,
 * because the tokens that matter here are the *resolved* ones: the app's theme
 * switches by swapping custom properties on the root element, and a value
 * imported at build time would be whichever theme happened to be compiled.
 */

/** The zoom past which the map will not go. See `MeetAgreement`. */
export const MAX_ZOOM = 6;

/** Above this, individuals rather than clusters. */
export const CLUSTER_MAX_ZOOM = 5;

function token(name: string, fallback: string): string {
  if (typeof document === "undefined") return fallback;
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim();
  return value || fallback;
}

/**
 * The whole style, as MapLibre wants it.
 *
 * One `geojson` source for the world and one for the pins. The pin source
 * clusters, which is what keeps ten thousand people at sixty frames: MapLibre
 * runs supercluster in its worker, and the alternative — a DOM marker each —
 * is a `<div>` the browser repositions every frame and dies in the hundreds.
 */
export function mapStyle(): StyleSpecification {
  return {
    version: 8,
    // No glyph or sprite URL: both are network fetches, and nothing here needs
    // either. Labels come from the character sprites we add ourselves.
    sources: {
      world: { type: "geojson", data: countries() },
      pins: {
        type: "geojson",
        data: { type: "FeatureCollection", features: [] },
        cluster: true,
        clusterRadius: 50,
        clusterMaxZoom: CLUSTER_MAX_ZOOM,
      },
    },
    layers: [
      {
        id: "water",
        type: "background",
        paint: { "background-color": token("--color-surface-0", "#0e0e10") },
      },
      {
        id: "land",
        type: "fill",
        source: "world",
        paint: { "fill-color": token("--color-surface-2", "#1a1e24") },
      },
      {
        id: "borders",
        type: "line",
        source: "world",
        paint: {
          "line-color": token("--color-line", "#2a2f36"),
          "line-width": 0.6,
        },
      },
      // Clusters: a circle that grows with the count. Drawn instead of the
      // characters, not as well as them — the filter below is the pair.
      {
        id: "clusters",
        type: "circle",
        source: "pins",
        filter: ["has", "point_count"],
        paint: {
          "circle-color": token("--color-accent", "#7c6cf0"),
          "circle-opacity": 0.85,
          "circle-radius": [
            "step",
            ["get", "point_count"],
            14,
            10,
            18,
            50,
            24,
            250,
            30,
          ],
          "circle-stroke-width": 1,
          "circle-stroke-color": token("--color-surface-0", "#0e0e10"),
        },
      },
      {
        id: "cluster-count",
        type: "symbol",
        source: "pins",
        filter: ["has", "point_count"],
        layout: {
          "text-field": ["get", "point_count_abbreviated"],
          "text-size": 12,
          "text-allow-overlap": true,
        },
        paint: { "text-color": token("--color-surface-0", "#0e0e10") },
      },
      // Individuals. `icon-image` names a sprite added at runtime from the
      // person's own character; `MeetMap` keeps that atlas.
      {
        id: "people",
        type: "symbol",
        source: "pins",
        filter: ["!", ["has", "point_count"]],
        layout: {
          "icon-image": ["get", "charId"],
          "icon-size": 1,
          "icon-allow-overlap": true,
          // Anchored at the bottom: a full-body character stands on its pin
          // rather than floating with its middle on it.
          "icon-anchor": "bottom",
        },
      },
    ],
  };
}
