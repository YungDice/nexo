import { useCallback, useEffect, useState } from "react";

import { Avatar } from "../../components/ui/Avatar";
import { Callout } from "../../components/ui/Feedback";
import { Modal } from "../../components/ui/Modal";
import {
  asMeetError,
  listStories,
  openStory,
  type Story,
} from "../../lib/meet";
import { pickAndPostStory } from "./story";

/**
 * The stories strip: other people's stories, on Home.
 *
 * On Home because a story's audience is *contacts* — people you already share
 * a conversation with — and Home is already where other people's things
 * appear.
 *
 * **Posting is not here.** It lives on your profile, with the other things
 * that are yours and that other people see: your picture, your banner, your
 * bio. A `+` sitting among other people's stories reads as "add to this row",
 * which is not what it does.
 *
 * Two honesty points the UI has to carry, both from `docs/THREAT-MODEL.md`:
 *
 *  - A story is **public to your contacts and readable by nobody else**, but
 *    somebody who was allowed to see it can keep it. The composer says so
 *    before anything is posted, not afterwards.
 *  - The 24 hours are real on this device: opening the strip is what deletes
 *    expired stories *and their keys*. So the count here is the truth about
 *    what still exists locally, not a filtered view of something retained.
 */
export function Stories({ canPost = false }: { canPost?: boolean }) {
  const [stories, setStories] = useState<Story[] | null>(null);
  const [viewing, setViewing] = useState<{ story: Story; src: string } | null>(
    null,
  );
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  // Reading is also the purge. Doing it on mount means the expired ones go
  // whenever somebody opens Home, with no timer anywhere.
  const load = useCallback(async () => {
    try {
      setStories(await listStories());
      setProblem(null);
    } catch (error) {
      const e = asMeetError(error);
      // Not being signed in yet is not a failure worth a banner.
      if (e.kind !== "signed_out") setProblem(e.message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function add() {
    setBusy(true);
    try {
      const result = await pickAndPostStory();
      if (result.posted) await load();
      else if (result.problem) setProblem(result.problem);
    } finally {
      setBusy(false);
    }
  }

  async function view(story: Story) {
    setBusy(true);
    try {
      setViewing({ story, src: await openStory(story.id) });
    } catch (error) {
      setProblem(asMeetError(error).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="mb-4">
      <div className="flex items-center gap-3 overflow-x-auto pb-1">
        {canPost ? (
          <button
            type="button"
            onClick={() => void add()}
            disabled={busy}
            className="border-line hover:border-accent flex h-14 w-14 shrink-0 items-center justify-center rounded-full border-2 border-dashed transition-colors"
            aria-label="Post a story"
            title="Post a story — your contacts can see it for 24 hours"
          >
            <span className="text-text-lo text-[20px] leading-none">+</span>
          </button>
        ) : null}

        {stories?.map((story) => (
          <button
            key={story.id}
            type="button"
            onClick={() => void view(story)}
            disabled={busy}
            className="flex shrink-0 flex-col items-center gap-1"
            aria-label={`Story from ${story.author_handle || "a contact"}`}
          >
            <span className="ring-accent rounded-full ring-2 ring-offset-2 ring-offset-[var(--color-surface-1)]">
              <Avatar
                seed={story.author_handle || story.author_device_id}
                name={story.author_handle || "?"}
                size={52}
              />
            </span>
            <span className="text-text-lo max-w-[64px] truncate text-[11px]">
              {/* Empty for a story that arrived over the wire: an envelope
                  names a device, not an account. Better a dash than a UUID. */}
              {story.author_handle || "—"}
            </span>
          </button>
        ))}

        {stories && stories.length === 0 ? (
          <p className="text-text-lo text-meta">
            No stories. Yours lasts 24 hours and goes to the people you already
            have a conversation with.
          </p>
        ) : null}
      </div>

      {problem ? (
        <Callout tone="warning" icon="alert" className="mt-2">
          {problem}
        </Callout>
      ) : null}

      {viewing ? (
        <Modal
          label={`Story from ${viewing.story.author_handle || "a contact"}`}
          onClose={() => setViewing(null)}
        >
          <div className="rounded-panel bg-surface-2 border-line max-w-[560px] border p-3">
            {/* A `data:` URL from Rust. Nothing remote is fetched — rule 3.
                The kind comes from the URL itself, which Rust built from the
                *sniffed* type rather than from anything the sender claimed. */}
            {viewing.src.startsWith("data:video/") ? (
              <video
                src={viewing.src}
                controls
                autoPlay
                className="rounded-control max-h-[70vh] w-auto"
              />
            ) : (
              <img
                src={viewing.src}
                alt=""
                className="rounded-control max-h-[70vh] w-auto"
              />
            )}
            <p className="text-text-lo mt-2 text-meta">
              Gone {new Date(viewing.story.expires_at_ms).toLocaleString()} —
              from this device and from the server. Someone who has already seen
              it can still have kept it.
            </p>
          </div>
        </Modal>
      ) : null}
    </section>
  );
}
