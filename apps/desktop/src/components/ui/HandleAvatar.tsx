import { useEffect, useSyncExternalStore } from "react";

import { knownProfile, loadProfile, subscribeProfiles } from "../../lib/profiles";
import { Avatar } from "./Avatar";
import { RemoteImage } from "./RemoteImage";

/**
 * Somebody's avatar, found from their handle.
 *
 * The generated gradient is not a placeholder to be apologised for — it is what
 * an account without a picture looks like, and it is what shows while the
 * lookup is in flight and if the lookup fails. Nothing here waits: the initials
 * are drawn immediately and the picture replaces them when it arrives.
 */
export function HandleAvatar({
  handle,
  name,
  size = 36,
  className,
}: {
  handle: string;
  /** Shown as initials until the real display name is known. */
  name?: string;
  size?: number;
  className?: string;
}) {
  const profile = useSyncExternalStore(
    subscribeProfiles,
    () => knownProfile(handle),
    () => undefined,
  );

  useEffect(() => {
    if (!handle) return;
    // Failures are swallowed: a missing profile means the generated avatar
    // stands, which is the same thing the account would look like anyway.
    void loadProfile(handle).catch(() => {});
  }, [handle]);

  const label = profile?.display_name ?? name ?? handle;

  if (profile?.avatar_key) {
    return (
      <RemoteImage
        imageKey={profile.avatar_key}
        alt={label}
        className={`shrink-0 rounded-full ${className ?? ""}`.trim()}
        style={{ width: size, height: size }}
      />
    );
  }

  return <Avatar seed={handle} name={label} size={size} {...(className ? { className } : {})} />;
}
