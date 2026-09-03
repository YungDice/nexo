import { useState } from "react";

import { Avatar } from "../../components/ui/Avatar";
import { Callout } from "../../components/ui/Feedback";
import { StoryViewer } from "./StoryViewer";
import { pickAndPostStory } from "./story";
import { groupStories, type StoryGroup } from "./storyGroups";
import { useStories } from "./useStories";

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
 * where somebody posted more than once; tapping opens that person's stories
 * (`StoryViewer`) in the order they happened, with Prev/Next between them.
 *
 * The honesty point the UI has to carry, from `docs/THREAT-MODEL.md`: a story
 * is **public to your contacts and readable by nobody else**, but somebody who
 * was allowed to see it can keep it. The composer says so before anything is
 * posted, not afterwards.
 */
export function Stories({ canPost = false }: { canPost?: boolean }) {
  const { stories, refresh, problem: loadProblem } = useStories();
  const [viewing, setViewing] = useState<StoryGroup | null>(null);
  const [busy, setBusy] = useState(false);
  const [postProblem, setPostProblem] = useState<string | null>(null);

  async function add() {
    setBusy(true);
    try {
      const result = await pickAndPostStory();
      if (result.posted) await refresh();
      else if (result.problem) setPostProblem(result.problem);
    } finally {
      setBusy(false);
    }
  }

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
              onClick={() => setViewing(group)}
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

      {loadProblem || postProblem ? (
        <Callout tone="warning" icon="alert" className="mt-2">
          {postProblem ?? loadProblem}
        </Callout>
      ) : null}

      {viewing ? (
        <StoryViewer group={viewing} onClose={() => setViewing(null)} />
      ) : null}
    </section>
  );
}
