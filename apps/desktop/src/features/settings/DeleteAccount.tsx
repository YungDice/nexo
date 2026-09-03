import { useState } from "react";

import { useApp } from "../../app/store";
import { Button } from "../../components/ui/Button";
import { Field } from "../../components/ui/Controls";
import { Callout } from "../../components/ui/Feedback";
import { Modal } from "../../components/ui/Modal";
import { asAuthError, deleteAccount } from "../../lib/auth";

/**
 * Deleting the account.
 *
 * The one action in this app that cannot be undone by anybody. There is no
 * recovery by design — the server holds ciphertext it cannot read and nothing
 * that could send a reset — so the usual "are you sure?" is not enough
 * ceremony. Two different things are asked for, and they are doing two
 * different jobs:
 *
 * - **The handle**, typed out. This is the deliberateness gate. It cannot be
 *   answered by reflex, and it names the account out loud, so nobody deletes
 *   the wrong one.
 * - **The password**, checked by the server. This is authorisation.
 *   `change-password` already establishes the rule this follows: a bearer
 *   token is possession of a session, not knowledge of the password, and an
 *   unattended unlocked machine must not be enough. It matters more here,
 *   because a wrong guess costs nothing and a right one costs everything.
 *
 * The password never leaves Rust as a password: it becomes a verifier there,
 * against the salt for this handle, and only the verifier is sent.
 */
export function DeleteAccount() {
  const account = useApp((s) => s.account);
  const setAccount = useApp((s) => s.setAccount);
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!account) return null;

  // Exact, not case-insensitive and not trimmed at the edges by the check:
  // handles are lowercase by the rules in §4.1, so anything else is a person
  // who has not read the sentence above the box.
  const confirmed = typed === account.handle;

  function close() {
    setOpen(false);
    setTyped("");
    setPassword("");
    setError(null);
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!account || !confirmed || !password || busy) return;
    setBusy(true);
    setError(null);
    try {
      await deleteAccount(account.handle, password);
      // The account is gone on the server and this machine's store with it.
      // Dropping the session is what puts the sign-in screen back up; there is
      // nothing left for the shell to draw.
      setAccount(null);
    } catch (raw) {
      // Nothing has been deleted when this happens — the core talks to the
      // server before it touches anything local, precisely so that a refusal
      // here is a refusal and not half a deletion.
      setError(asAuthError(raw).message);
      setBusy(false);
    }
  }

  return (
    <>
      <div className="py-3">
        <Callout tone="danger" icon="alert" title="Deleting your account cannot be undone.">
          Your profile, posts, comments and reactions go, and so does every message this
          device holds. Messages you already sent stay on the machines they reached — the
          server never had the keys and cannot take them back. Anything you sent that has
          not been picked up yet will never arrive.
        </Callout>
        <div className="mt-3">
          <Button variant="danger" icon="trash" onClick={() => setOpen(true)}>
            Delete account
          </Button>
        </div>
      </div>

      {open ? (
        <Modal label="Delete your account" onClose={close}>
          <form
            onSubmit={submit}
            className="rounded-panel bg-surface-2 w-full max-w-[380px] border border-line p-5"
          >
            <h2 className="text-text-hi font-display text-[17px] font-medium">
              Delete your account
            </h2>
            <p className="text-text-lo mt-1.5 text-meta">
              This is permanent. There is no recovery, no reset link, and nobody who can put
              it back — not even us.
            </p>

            <Field
              label={`Type ${account.handle} to confirm`}
              className="mt-4"
              value={typed}
              autoFocus
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => setTyped(e.target.value)}
            />

            <Field
              label="Your password"
              type="password"
              className="mt-3"
              value={password}
              autoComplete="current-password"
              hint="Asked for because a signed-in session is not the same as knowing the password."
              onChange={(e) => setPassword(e.target.value)}
            />

            {error ? (
              <Callout tone="danger" icon="alert" className="mt-3">
                {error}
              </Callout>
            ) : null}

            <div className="mt-4 flex gap-2">
              <Button
                type="submit"
                variant="danger"
                disabled={!confirmed || !password || busy}
              >
                {busy ? "Deleting…" : "Delete my account"}
              </Button>
              <Button type="button" onClick={close} disabled={busy}>
                Cancel
              </Button>
            </div>
          </form>
        </Modal>
      ) : null}
    </>
  );
}
