import type { CSSProperties } from "react";
import { useEffect, useState } from "react";

import { cn } from "../../lib/cn";
import { imageUrl } from "../../lib/feed";
import { fieldFor } from "../../lib/palette";

/**
 * An image stored in object storage, rendered from its key.
 *
 * The bucket is private (§5.3), so there is no URL to put in a `src` — every
 * read goes through a presigned GET with a 60-minute life. This component asks
 * Rust for one, remembers it, and draws the image.
 *
 * A generated field stands in while the URL is on its way and stays if it never
 * arrives, so a feed with a dead object still lays out correctly instead of
 * collapsing to zero height or showing a broken-image glyph.
 *
 * # A note on caching
 *
 * URLs are memoised per key for the process's lifetime, because a feed scrolled
 * back and forth would otherwise presign the same object repeatedly. They are
 * not persisted: a presigned URL is a bearer credential for one object, and
 * writing a pile of them to disk to save a few round trips would be trading a
 * real property for a small one.
 */
const urls = new Map<string, Promise<string>>();

function presign(key: string): Promise<string> {
  const existing = urls.get(key);
  if (existing) return existing;
  const pending = imageUrl(key);
  urls.set(key, pending);
  // A failure must not be cached: the next render should try again rather than
  // inherit a rejected promise forever.
  void pending.catch(() => urls.delete(key));
  return pending;
}

export function RemoteImage({
  imageKey,
  alt,
  className,
  style,
}: {
  /** The object key, e.g. `media/42/{uuid}`. */
  imageKey: string;
  alt: string;
  className?: string;
  /** Exact dimensions, for callers sized in pixels rather than in classes. */
  style?: CSSProperties;
}) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setUrl(null);
    void presign(imageKey)
      .then((next) => {
        if (!cancelled) setUrl(next);
      })
      .catch(() => {
        // Left as the placeholder field. Rule 7 says a failure is shown, and
        // here the honest showing is "this image is not available" rather than
        // an error dialog for something nobody asked to open.
      });
    return () => {
      cancelled = true;
    };
  }, [imageKey]);

  if (!url) {
    return (
      <div
        role="img"
        aria-label={alt}
        className={cn(className)}
        style={{ background: fieldFor(imageKey), ...style }}
      />
    );
  }

  return (
    <div
      role="img"
      aria-label={alt}
      className={cn("bg-cover bg-center", className)}
      style={{ backgroundImage: `url(${url})`, ...style }}
    />
  );
}
