import { cn } from "../../lib/cn";
import { initials } from "../../lib/format";
import { gradientFor } from "../../lib/palette";
import type { PresenceState } from "../../lib/types";

const presenceColor: Record<PresenceState, string> = {
  online: "bg-success",
  away: "bg-warning",
  offline: "bg-text-lo",
};

export interface AvatarProps {
  /** Stable per person or per group, so the colours never move under you. */
  seed: string;
  name: string;
  size?: number;
  presence?: PresenceState;
  className?: string;
  /** A locally picked picture, already converted to a WebView-loadable URL. */
  imageUrl?: string;
}

/**
 * A generated avatar.
 *
 * The CSP allows images from 'self', asset:, data: and blob: only — there is
 * no remote host to load a photo from, and that is deliberate (§4.5). Until a
 * real avatar has been picked from disk (`imageUrl`), an account is a
 * gradient and two initials derived from its handle.
 */
export function Avatar({ seed, name, size = 40, presence, className, imageUrl }: AvatarProps) {
  const dot = Math.max(8, Math.round(size * 0.26));
  return (
    <span
      className={cn("relative inline-flex shrink-0", className)}
      style={{ width: size, height: size }}
    >
      <span
        aria-hidden="true"
        className="flex size-full items-center justify-center rounded-full bg-cover bg-center font-medium text-white ring-1 ring-line-strong"
        style={
          imageUrl
            ? { backgroundImage: `url(${imageUrl})` }
            : { background: gradientFor(seed), fontSize: Math.round(size * 0.36) }
        }
      >
        {imageUrl ? null : initials(name)}
      </span>
      {presence ? (
        <span
          className={cn(
            "absolute right-0 bottom-0 rounded-full ring-2 ring-surface-1",
            presenceColor[presence],
          )}
          style={{ width: dot, height: dot }}
          role="img"
          aria-label={`${name} is ${presence}`}
        />
      ) : null}
    </span>
  );
}
