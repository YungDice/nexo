import { useEffect } from "react";

import { checkUpdate, installUpdate, notify } from "../lib/native";

/**
 * Looks for a new version at launch and installs it.
 *
 * The point is that nobody has to fetch a build by hand. It runs once, early,
 * and before the app is being used for anything: an install restarts the
 * process, and doing that in the middle of typing a message would lose it.
 *
 * Failure is deliberately silent. An unreachable release host, a dev build with
 * no key configured, being offline — none of those are things the person who
 * just opened a messenger can act on, and an error box in front of the app on
 * every launch without a network is worse than shipping a version late. The one
 * thing that is announced is the update actually happening, because the window
 * is about to disappear and come back.
 *
 * The signature is checked by the updater plugin against the key pinned in
 * `tauri.conf.json` before anything is run, so a release host that starts
 * serving something else cannot get a build past this.
 */
export function useAutoUpdate(enabled: boolean): void {
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;

    void (async () => {
      try {
        const update = await checkUpdate();
        if (cancelled || !update) return;

        await notify("Updating Nexo", `Installing version ${update.version}. The app will restart.`);
        // Does not return: the process restarts into the new version.
        await installUpdate();
      } catch {
        // Nothing to tell the user. See above.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [enabled]);
}
