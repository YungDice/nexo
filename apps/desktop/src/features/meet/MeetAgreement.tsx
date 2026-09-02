import { Button } from "../../components/ui/Button";
import { Callout } from "../../components/ui/Feedback";

/**
 * What somebody agrees to before they appear on the map.
 *
 * Every sentence below is checked against what was actually built, which is
 * the only thing that makes it worth showing. Rule 5 is not "write a
 * reassuring paragraph" — it is that a feature which implies privacy it does
 * not have is worse than one that never claimed any, because the person acted
 * on the claim.
 *
 * Specifically:
 *
 *  - *"the server can read all three"* — `meet_profiles` stores the pin, the
 *    headline and the character as ordinary columns. No encryption anywhere.
 *  - *"not a location we measured"* — there is no `navigator.geolocation` call
 *    in this feature and no column that could hold an accuracy.
 *  - *"does not zoom in far enough to point at a building"* — `MAX_ZOOM` is 6.
 *  - *"deliberately imprecise"* — `meet::coarsen` snaps to a 0.25° grid and
 *    offsets it before anything is written.
 *  - *"blocking removes you from each other's map"* — `/v1/meet/pins` spends
 *    `blocks::hidden_authors`, in both directions, and there is a test for it.
 *
 * Never "military grade". Never "anonymous". If one of these sentences stops
 * being true of the code, it is the sentence that is wrong.
 */
export function MeetAgreement({
  onAccept,
  busy,
}: {
  onAccept: () => void;
  busy?: boolean;
}) {
  return (
    <div className="mx-auto flex h-full max-w-[560px] flex-col justify-center gap-5 px-6">
      <h1 className="text-text-hi font-display text-[22px] font-medium">
        Meet&amp;Greet is public.
      </h1>

      <Callout tone="warning" icon="alert">
        Your pin, your character and your headline are visible to every
        logged-in Nexo user, and the server can read all three. They are not
        encrypted. This is the same deal as your profile and your posts.
      </Callout>

      <div className="text-text-body flex flex-col gap-3 text-[13px] leading-relaxed">
        <p>
          <strong className="text-text-hi">Nexo does not know where you are.</strong>{" "}
          Your pin is a place you chose on a map, not a location we measured.
          Nexo never reads your device&rsquo;s location, and the map does not
          zoom in far enough to point at a building &mdash; the pin we store is
          deliberately imprecise, and it does not move unless you move it.
        </p>
        <p>
          Messages you go on to exchange are end-to-end encrypted, like every
          other conversation in Nexo. Who you talk to and when is visible to the
          server.
        </p>
        <p>
          You can leave the map at any time, and blocking someone removes you
          from each other&rsquo;s map in both directions.
        </p>
      </div>

      <div className="flex gap-2">
        <Button variant="primary" onClick={onAccept} disabled={busy}>
          {busy ? "One moment…" : "I understand — put me on the map"}
        </Button>
      </div>
    </div>
  );
}
