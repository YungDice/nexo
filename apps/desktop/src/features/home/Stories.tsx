import { useCallback, useEffect, useState } from "react";

import { Avatar } from "../../components/ui/Avatar";
import { Icon } from "../../components/ui/Icon";
import { cn } from "../../lib/cn";
import { Callout } from "../../components/ui/Feedback";
import { Modal } from "../../components/ui/Modal";
import {
  asMeetError,
  listStories,
  openStory,
  type Story,
} from "../../lib/meet";
import { pickAndPostStory } from "./story";
import { groupStories, type StoryGroup } from "./storyGroups";

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
 * **One circle per person, not per story.** `listStories()` is a flat list —
 * every live story this device holds, own and received mixed together — and
 * this used to draw it exactly as it arrived: two posts from the same person
 * were two circles, indistinguishable from two different people. They are
 * grouped now (`groupStories`), with your own circle leading and a `×n` badge
 * where somebody posted more than once; tapping opens that person's stories in
 * the order they happened, with Prev/Next between them.
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
  const [viewing, setViewing] = useState<{
    group: StoryGroup;
    index: number;
  } | null>(null);
  const [src, setSrc] = useState<string | null>(null);
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

  const current = viewing ? viewing.group.stories[viewing.index] : undefined;

  // Fetches whichever story `viewing` now points at. Runs on open and on
  // every Prev/Next, rather than pre-fetching a whole group up front: most
  // groups have one story, and the ones that do not are watched one at a
  // time anyway.
  useEffect(() => {
    if (!current) return;
    let cancelled = false;
    setSrc(null);
    setBusy(true);
    void openStory(current.id)
      .then((next) => {
        if (!cancelled) setSrc(next);
      })
      .catch((error) => {
        if (!cancelled) setProblem(asMeetError(error).message);
      })
      .finally(() => {
        if (!cancelled) setBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [current]);

  const step = useCallback((by: number) => {
    setViewing((v) => {
      if (!v) return v;
      const next = v.index + by;
      if (next < 0 || next >= v.group.stories.length) return v;
      return { group: v.group, index: next };
    });
  }, []);

  useEffect(() => {
    if (!viewing) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "ArrowRight") step(1);
      else if (event.key === "ArrowLeft") step(-1);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [viewing, step]);

  const groups = groupStories(stories ?? []);

  return (
    <section className="mb-4">
      {/* `py-2 px-1` is not decoration: the ring below is drawn with
          `ring-offset`, which extends past the avatar's own box. A scroll
          container with no room around its content clips that overflow —
          `overflow-x: auto` forces the vertical axis to clip too, per the CSS
          overflow spec, so the top and bottom of the ring were cut off, and
          the first and last circle lost their outer edge to the row's own
          bounds. */}
      <div className="flex items-center gap-3 overflow-x-auto px-1 py-2">
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

        {groups.map((group) => {
          const count = group.stories.length;
          const name = group.mine
            ? "Your story"
            : group.authorHandle || "a contact";
          return (
            <button
              key={group.key}
              type="button"
              onClick={() => setViewing({ group, index: 0 })}
              disabled={busy}
              className="flex shrink-0 flex-col items-center gap-1"
              aria-label={
                count > 1
                  ? `${count} stories from ${name}`
                  : `Story from ${name}`
              }
            >
              <span className="relative">
                <span className="ring-accent rounded-full ring-2 ring-offset-2 ring-offset-[var(--color-surface-1)]">
                  <Avatar
                    seed={group.authorHandle || group.key}
                    name={group.mine ? "You" : group.authorHandle || "?"}
                    size={52}
                  />
                </span>
                {count > 1 ? (
                  <span
                    aria-hidden="true"
                    className="bg-accent absolute -right-0.5 -bottom-0.5 rounded-full px-1.5 py-0.5 font-mono text-[10px] leading-none text-white"
                  >
                    ×{count}
                  </span>
                ) : null}
              </span>
              <span className="text-text-lo max-w-[64px] truncate text-[11px]">
                {/* Empty a moment longer than usual only offline, or right
                    after a story arrives and before the listing has caught
                    up -- see `Story.author_handle`. Better a dash than a
                    UUID either way. */}
                {group.mine ? "You" : group.authorHandle || "—"}
              </span>
            </button>
          );
        })}

        {groups.length === 0 && stories ? (
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

      {viewing && current ? (
        <Modal
          label={
            viewing.group.mine
              ? "Your story"
              : `Story from ${viewing.group.authorHandle || "a contact"}`
          }
          onClose={() => setViewing(null)}
        >
          <div className="relative">
            {viewing.index > 0 ? (
              <button
                type="button"
                aria-label="Previous"
                onClick={(e) => {
                  e.stopPropagation();
                  step(-1);
                }}
                className="absolute top-1/2 -left-4 z-10 -translate-x-full -translate-y-1/2 rounded-full bg-black/50 p-2 text-white hover:bg-black/70"
              >
                <Icon name="chevronLeft" size={20} />
              </button>
            ) : null}

            <div className="rounded-panel bg-surface-2 border-line max-w-[560px] border p-3">
              {viewing.group.stories.length > 1 ? (
                <div className="mb-2 flex gap-1" aria-hidden="true">
                  {viewing.group.stories.map((s, i) => (
                    <span
                      key={s.id}
                      className={cn(
                        "h-[3px] flex-1 rounded-full",
                        i <= viewing.index ? "bg-accent" : "bg-line",
                      )}
                    />
                  ))}
                </div>
              ) : null}

              {!src ? (
                <div
                  className="flex h-[240px] w-[300px] items-center justify-center"
                  aria-label="Loading"
                  role="img"
                >
                  <span className="text-text-lo text-meta">Decrypting…</span>
                </div>
              ) : /* A `data:` URL from Rust. Nothing remote is fetched — rule 3.
                     The kind comes from the URL itself, which Rust built from the
                     *sniffed* type rather than from anything the sender claimed. */
              src.startsWith("data:video/") ? (
                <video
                  src={src}
                  controls
                  autoPlay
                  className="rounded-control max-h-[70vh] w-auto"
                />
              ) : (
                <img
                  src={src}
                  alt=""
                  className="rounded-control max-h-[70vh] w-auto"
                />
              )}
              <p className="text-text-lo mt-2 text-meta">
                Gone {new Date(current.expires_at_ms).toLocaleString()} — from
                this device and from the server. Someone who has already seen
                it can still have kept it.
              </p>
            </div>

            {viewing.index < viewing.group.stories.length - 1 ? (
              <button
                type="button"
                aria-label="Next"
                onClick={(e) => {
                  e.stopPropagation();
                  step(1);
                }}
                className="absolute top-1/2 -right-4 z-10 -translate-y-1/2 translate-x-full rounded-full bg-black/50 p-2 text-white hover:bg-black/70"
              >
                <Icon name="chevronLeft" size={20} className="rotate-180" />
              </button>
            ) : null}
          </div>
        </Modal>
      ) : null}
    </section>
  );
}
