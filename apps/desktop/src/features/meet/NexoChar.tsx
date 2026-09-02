import { useMemo } from "react";

import { Avatar, Style } from "@dicebear/core";
// The style by its own subpath, never a barrel: `@dicebear/styles` exports one
// entry per style, and this feature needs exactly one of the sixty-one. The
// package maps this specifier to its own `.min.json`, so the minified
// definition is what lands in the chunk.
import voxelArt from "@dicebear/styles/voxel-art.json";

/**
 * A NexoChar: somebody's character, rendered from its config.
 *
 * Three properties of the generator shape everything around this file.
 *
 * **It is deterministic and offline.** The same config produces the same SVG
 * bytes, every time, with no network call. That is what makes it safe to store
 * a config instead of a picture.
 *
 * **What is stored is the config, never the image.** A couple of hundred bytes
 * of JSON, rendered by whichever client is drawing it. No object storage, no
 * presigned URL, nothing for `media.rs` to carry, and no picture that would
 * have to be moderated as an image.
 *
 * **Nothing animates.** `animationProbability: 0` everywhere: an animated SVG
 * on a map pin is a repaint per frame per pin, and there can be thousands.
 */

/** The style, built once. Construction validates the definition. */
export const VOXEL_ART = new Style(voxelArt);

/** A character config. Opaque everywhere except here. */
export type CharConfig = Record<string, unknown>;

/** Options every render forces, whatever the config says. */
const FORCED: CharConfig = {
  // See above: a moving pin is a repaint per frame.
  animationProbability: 0,
};

/** Renders a config to SVG markup. Deterministic, and never a network call. */
export function renderChar(config: CharConfig): string {
  return new Avatar(VOXEL_ART, { ...config, ...FORCED } as never).toString();
}

/**
 * A character, drawn.
 *
 * The SVG is generated in this process from a config that came over IPC as
 * data, so there is no remote document here and nothing for rule 3 to object
 * to — but it is still markup being injected, so `title` is the only thing
 * around it that a caller controls.
 */
export function NexoChar({
  config,
  size = 64,
  className,
  title,
}: {
  config: CharConfig;
  size?: number;
  className?: string;
  /** For screen readers. The character itself is decorative. */
  title?: string;
}) {
  const svg = useMemo(() => renderChar(config), [config]);

  return (
    <span
      className={className}
      style={{ width: size, height: size, display: "inline-block" }}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      // Generated locally from a validated style definition, in this process,
      // by a library that emits SVG and nothing else.
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
