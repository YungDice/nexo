import { useCallback, useEffect, useMemo, useState } from "react";
import { totalUnread, useApp } from "./app/store";
import { startSyncAgent } from "./app/syncAgent";
import { useAutoLock } from "./app/useAutoLock";
import { useLayout } from "./app/useLayout";
import { useMaximized } from "./app/useWindow";
import { IconRail } from "./components/chrome/IconRail";
import { PageTitleCell, TopBar } from "./components/chrome/TopBar";
import { IconButton } from "./components/ui/Button";
import { HomePage } from "./features/home/HomePage";
import { MeetPage } from "./features/meet/MeetPage";
import { MessagesHeader } from "./features/messages/MessagesHeader";
import { MessagesPage } from "./features/messages/MessagesPage";
import { ProfilePage } from "./features/profile/ProfilePage";
import { PublicProfile } from "./features/profile/PublicProfile";
import { AuthPage } from "./features/auth/AuthPage";
import { LockScreen } from "./features/auth/LockScreen";
import { RequirePin } from "./features/auth/RequirePin";
import { SettingsPage } from "./features/settings/SettingsPage";
import { pinStatus, restoreSession, type Account } from "./lib/auth";
import { myProfile } from "./lib/feed";
import { notify, setCloseToTray } from "./lib/native";
import { DialogHost } from "./components/ui/DialogHost";
import { useAutoUpdate } from "./app/useAutoUpdate";
import { useChrome } from "./app/useChrome";

/**
 * The app shell (§7.3).
 *
 * A frameless window whose whole chrome is one card: a single top row carrying
 * the wordmark, the account, the conversation and the caption buttons, then
 * the 64px rail and one of four destinations. The panes float on `app-field`,
 * a soft neutral gradient, which is what makes the glass visible at all:
 * `backdrop-filter` blurs whatever is behind a pane, and until the field
 * existed what was behind every pane was one flat fill, so the blur returned
 * the colour it started with. Nothing opaque may be added between the field
 * and the panes without taking the effect with it — and when the window
 * backdrop is on, the field itself is a veil over the desktop, so "nothing
 * opaque" now reaches all the way out to `body` (see `useChrome`).
 *
 * There is no router: four destinations and no deep links do not need one, and
 * §7.4 asks for no page transitions anyway. When the feed grows permalinks, a
 * router goes in here and nothing below it changes.
 *
 * `now` is created once and passed down. Everything that renders a time takes
 * it as an argument, so no component reaches for the clock on its own and
 * "yesterday" cannot mean two different days in two panes.
 */

function AppShell({ account }: { account: Account }) {
  const route = useApp((s) => s.route);
  const viewingHandle = useApp((s) => s.viewingHandle);
  const closeToTray = useApp((s) => s.preferences.closeToTray);
  const homeChat = useApp((s) => s.preferences.homeChat);
  const setPreference = useApp((s) => s.setPreference);
  const unreadLedger = useApp((s) => s.unread);
  const maximized = useMaximized();
  const layout = useLayout();

  const now = useMemo(() => new Date(), []);
  const unread = totalUnread(unreadLedger);

  // The one sync loop (M8): flush the offline queue, pull, badge, toast. It
  // lives for as long as someone is signed in and stops with the shell.
  useEffect(() => startSyncAgent(), []);

  // §8: after N minutes idle, Rust drops the store and the lock screen takes
  // over (the shell's parent draws it instead of this tree).
  useAutoLock();

  // The close handler runs in Rust and defaults to quit; the stored preference
  // is pushed across at startup and whenever Settings changes it.
  useEffect(() => {
    void setCloseToTray(closeToTray);
  }, [closeToTray]);

  void account;

  return (
    <div className="relative h-full overflow-hidden">
      <div className="app-field absolute inset-0 flex flex-col overflow-hidden">
        <TopBar maximized={maximized}>
          {route === "messages" ? <MessagesHeader now={now} /> : null}
          {route === "home" ? (
            <PageTitleCell
              title="Home"
              actions={
                <>
                  <IconButton
                    name="refresh"
                    label="Refresh the feed"
                    size={17}
                    onClick={() => void notify("Feed refreshed", "You're caught up — there's nothing new.")}
                  />
                  {/* Only offered where it would fit. Below the breakpoint the
                      panel is hidden regardless, and a toggle that changes
                      nothing visible is worse than no toggle. */}
                  {layout.canShowContext ? (
                    <IconButton
                      name="panel"
                      label={homeChat ? "Hide the conversation" : "Show the most recent conversation"}
                      size={17}
                      active={homeChat}
                      onClick={() => setPreference("homeChat", !homeChat)}
                    />
                  ) : null}
                </>
              }
            />
          ) : null}
          {route === "meet" ? <PageTitleCell title="Meet&Greet" /> : null}
          {route === "profile" ? <PageTitleCell title="Profile" /> : null}
          {route === "settings" ? <PageTitleCell title="Settings" /> : null}
        </TopBar>

        <div className="flex min-h-0 flex-1">
          <IconRail unread={unread} />
          {route === "home" ? <HomePage now={now} /> : null}
          {route === "meet" ? <MeetPage /> : null}
          {route === "messages" ? <MessagesPage now={now} /> : null}
          {route === "profile" ? (
            viewingHandle ? (
              <PublicProfile handle={viewingHandle} now={now} />
            ) : (
              <ProfilePage now={now} />
            )
          ) : null}
          {route === "settings" ? <SettingsPage now={now} /> : null}
        </div>
      </div>
    </div>
  );
}

/**
 * The session gate.
 *
 * Three states, and the middle one matters: until `restore_session` answers we
 * render neither the app nor the login form, because flashing a login screen at
 * someone who is already signed in is both alarming and wrong.
 *
 * The restore call deliberately does not touch the network (see
 * `session::restore` in `crates/client`), so a machine that is offline still
 * opens to its own account rather than to a sign-in prompt it cannot satisfy.
 */
export function App() {
  // Mirrored into the store as well as held here. The local copy drives the
  // sign-in/app switch below; the store copy is what the feed, the composer,
  // and the profile read without four levels of prop drilling.
  const [account, setLocalAccount] = useState<Account | null>(null);
  const setStoreAccount = useApp((s) => s.setAccount);
  const setAccount = useCallback(
    (next: Account | null) => {
      setLocalAccount(next);
      setStoreAccount(next);
    },
    [setStoreAccount],
  );
  const [checked, setChecked] = useState(false);

  // Theme, accent, depth and transparency, on the root element. Here rather
  // than in the shell because the sign-in form and the lock screen are drawn
  // from this component and are entitled to the same appearance.
  useChrome();

  // Only once the session gate has answered: an install restarts the process,
  // and doing that mid-sign-in would throw away a half-typed password.
  useAutoUpdate(checked);
  const locked = useApp((s) => s.locked);
  const storeAccount = useApp((s) => s.account);
  const setMyAvatarKey = useApp((s) => s.setMyAvatarKey);
  const setLocked = useApp((s) => s.setLocked);
  const maximized = useMaximized();

  useEffect(() => {
    let cancelled = false;
    void restoreSession()
      .then((found) => {
        if (!cancelled) setAccount(found);
      })
      .catch(() => {
        // An unreadable store is not a reason to hang on a blank window: fall
        // through to the sign-in form, which is the one thing that can fix it.
      })
      .finally(() => {
        if (!cancelled) setChecked(true);
      });
    return () => {
      cancelled = true;
    };
  }, [setAccount]);

  const onSignedIn = useCallback((next: Account) => setAccount(next), [setAccount]);

  // The signed-in person's own picture, fetched once per session.
  //
  // It lives on the profile, and until now only `useProfile` ever asked for it
  // -- a hook that only the profile page mounts. So anything else drawing
  // "you", the post composer above all, had nothing to draw and fell back to
  // the generated identicon, beside posts that showed the real face.
  //
  // Failure is silent on purpose: not knowing your avatar means the fallback,
  // which is what was drawn before anyway. It is not worth a banner.
  useEffect(() => {
    if (!storeAccount) return;
    let cancelled = false;
    void myProfile()
      .then((me) => {
        if (!cancelled) setMyAvatarKey(me.avatar_key);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [storeAccount, setMyAvatarKey]);

  // Whether this machine has an unlock PIN. `null` while the question is out.
  //
  // Asked for every signed-in account, not only new ones: an account that was
  // already signed in when the requirement arrived has the same unattended
  // machine to protect, and letting it through would make the rule apply to
  // whoever happened to install later.
  const [hasPin, setHasPin] = useState<boolean | null>(null);
  useEffect(() => {
    if (!account) {
      setHasPin(null);
      return;
    }
    let cancelled = false;
    void pinStatus()
      .then((status) => {
        if (!cancelled) setHasPin(status.set);
      })
      .catch(() => {
        // A keystore that will not answer is not a reason to hold someone out
        // of their own messages. Settings still asks for a PIN, loudly.
        if (!cancelled) setHasPin(true);
      });
    return () => {
      cancelled = true;
    };
  }, [account]);

  // Locked replaces the shell rather than covering it: the conversation tree
  // is unmounted, not hidden, so nothing readable sits underneath (§8).
  if (account && locked) {
    return (
      <div className="relative h-full overflow-hidden">
        <DialogHost />
        <div className="app-field absolute inset-0 flex flex-col overflow-hidden">
          <TopBar maximized={maximized} />
          <div className="min-h-0 flex-1">
            <LockScreen account={account} onUnlocked={() => setLocked(false)} />
          </div>
        </div>
      </div>
    );
  }

  // Before the shell, and after the lock screen: a locked app asks for the PIN
  // it already has, and only an unlocked one can be missing it.
  if (account && hasPin === false) {
    return (
      <div className="relative h-full overflow-hidden">
        <div className="app-field absolute inset-0 flex flex-col overflow-hidden">
          <TopBar maximized={maximized} />
          <div className="min-h-0 flex-1">
            <RequirePin account={account} onSet={() => setHasPin(true)} />
          </div>
        </div>
        <DialogHost />
      </div>
    );
  }

  if (account && hasPin !== null)
    return (
      <>
        <AppShell account={account} />
        <DialogHost />
      </>
    );

  return (
    <div className="relative h-full overflow-hidden">
      <div className="app-field absolute inset-0 flex flex-col overflow-hidden">
        {/* The window is frameless, so the titlebar has to exist before there
            is an account -- otherwise the window cannot be moved or closed. */}
        <TopBar maximized={maximized} />
        <div className="min-h-0 flex-1">
          {checked ? <AuthPage onSignedIn={onSignedIn} /> : null}
        </div>
      </div>
      <DialogHost />
    </div>
  );
}
