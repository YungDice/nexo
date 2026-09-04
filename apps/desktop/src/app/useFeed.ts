import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  asFeedError,
  createPost as createPostCall,
  deletePost as deletePostCall,
  feed as feedCall,
  react as reactCall,
  vote as voteCall,
  type FeedSort,
  type Post,
} from "../lib/feed";

/**
 * The Home feed, against the real server (§6.2).
 *
 * Cursor-paginated with infinite scroll, so this owns three things a component
 * should not: the accumulated pages, the cursor, and the "already loading"
 * flag that stops a fast scroll firing four overlapping requests for the same
 * page.
 *
 * Reactions are applied optimistically and reconciled from the server's reply.
 * A reaction is a one-tap action on a list that may be scrolling; waiting a
 * round trip to show it makes the tap feel like it missed. If the call fails
 * the post is restored and the error is shown — rule 7, no silent revert.
 */
export interface LiveFeed {
  posts: Post[];
  /** Non-null when something is wrong the user should see. */
  problem: string | null;
  /** True during the first load, when there is nothing to show yet. */
  loading: boolean;
  /** True while a further page is on its way. */
  loadingMore: boolean;
  /** False once the server stops handing back a cursor. */
  hasMore: boolean;
  loadMore: () => Promise<void>;
  refresh: () => Promise<void>;
  post: (input: NewPostInput) => Promise<void>;
  remove: (id: number) => Promise<void>;
  toggleReaction: (id: number, emoji: string) => Promise<void>;
  /** How the feed is ordered. Changing it starts the pages over. */
  sort: FeedSort;
  setSort: (sort: FeedSort) => void;
  /** Whether the feed is narrowed to accounts this person follows. */
  following: boolean;
  setFollowing: (following: boolean) => void;
  /** Casts, changes or withdraws a vote. Applied optimistically. */
  castVote: (id: number, value: number) => Promise<void>;
}

/** What the composer hands over. */
export interface NewPostInput {
  body: string;
  mediaKeys?: string[];
  title?: string | null;
  kind?: "text" | "link" | "image";
  linkUrl?: string | null;
}

export function useFeed(): LiveFeed {
  const [posts, setPosts] = useState<Post[]>([]);
  const [sort, setSortState] = useState<FeedSort>("new");
  // Which feed. Not persisted: which slice of the world somebody was reading
  // is session state, and opening the app into a filtered view they set days
  // ago is how a feed comes to look empty for no visible reason.
  const [following, setFollowingState] = useState(false);
  const [cursor, setCursor] = useState<number | null>(null);
  const [hasMore, setHasMore] = useState(true);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  // A ref rather than the state above: `loadMore` is called from a scroll
  // handler, and reading `loadingMore` there would see the value from the
  // render that installed the handler, not the current one.
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try {
      const page = await feedCall(undefined, sort, following);
      setPosts(page.posts);
      setCursor(page.next_cursor);
      setHasMore(page.next_cursor !== null);
      setProblem(null);
    } catch (error) {
      setProblem(asFeedError(error).message);
    } finally {
      inFlight.current = false;
      setLoading(false);
    }
  }, [sort, following]);

  const loadMore = useCallback(async () => {
    if (inFlight.current || cursor === null) return;
    inFlight.current = true;
    setLoadingMore(true);
    try {
      const page = await feedCall(cursor, sort, following);
      // Deduplicated on merge: a post deleted between two pages shifts the
      // window, and without this the same post can arrive twice and React
      // warns about duplicate keys.
      setPosts((current) => {
        const seen = new Set(current.map((p) => p.id));
        return [...current, ...page.posts.filter((p) => !seen.has(p.id))];
      });
      setCursor(page.next_cursor);
      setHasMore(page.next_cursor !== null);
      setProblem(null);
    } catch (error) {
      setProblem(asFeedError(error).message);
    } finally {
      inFlight.current = false;
      setLoadingMore(false);
    }
  }, [cursor, sort, following]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const post = useCallback(async (input: NewPostInput) => {
    try {
      const created = await createPostCall(input);
      // Prepended rather than refetched: the round trip already happened, and
      // making someone wait for a second one to see their own post is the kind
      // of delay that reads as "it didn't work".
      setPosts((current) => [created, ...current]);
      setProblem(null);
    } catch (error) {
      setProblem(asFeedError(error).message);
    }
  }, []);

  const remove = useCallback(async (id: number) => {
    const before = posts;
    setPosts((current) => current.filter((p) => p.id !== id));
    try {
      await deletePostCall(id);
      setProblem(null);
    } catch (error) {
      // Put it back. A post that vanishes from the screen but not the server
      // is worse than one that never vanished.
      setPosts(before);
      setProblem(asFeedError(error).message);
    }
  }, [posts]);

  const setSort = useCallback((next: FeedSort) => {
    // The cursor belongs to the old order and means nothing in the new one —
    // an id under `new`, an offset under the others. Clearing the pages is
    // what makes the change a fresh first page rather than a mixed list.
    setSortState((current) => {
      if (current === next) return current;
      setPosts([]);
      setCursor(null);
      setHasMore(true);
      setLoading(true);
      return next;
    });
  }, []);

  const setFollowing = useCallback((next: boolean) => {
    // Same reasoning as `setSort`: the cursor belongs to the old list, and
    // paging on with it would mix two feeds together.
    setFollowingState((current) => {
      if (current === next) return current;
      setPosts([]);
      setCursor(null);
      setHasMore(true);
      setLoading(true);
      return next;
    });
  }, []);

  const castVote = useCallback(async (id: number, value: number) => {
    const before = posts;
    // Optimistic, like reactions: a vote is a one-tap action on a list that
    // may be scrolling, and a round trip before the arrow moves reads as a
    // tap that missed.
    setPosts((current) =>
      current.map((p) =>
        p.id === id
          ? { ...p, score: p.score - p.my_vote + value, my_vote: value }
          : p,
      ),
    );
    try {
      const result = await voteCall(id, value);
      setPosts((current) =>
        current.map((p) =>
          p.id === id ? { ...p, score: result.score, my_vote: result.my_vote } : p,
        ),
      );
      setProblem(null);
    } catch (error) {
      // Rule 7: put it back and say so, never a silent revert.
      setPosts(before);
      setProblem(asFeedError(error).message);
    }
  }, [posts]);

  const toggleReaction = useCallback(async (id: number, emoji: string) => {
    const before = posts;
    const target = posts.find((p) => p.id === id);
    if (!target) return;
    const on = !target.my_reactions.includes(emoji);

    setPosts((current) =>
      current.map((p) => (p.id === id ? applyReaction(p, emoji, on) : p)),
    );

    try {
      const reactions = await reactCall(id, emoji, on);
      // The server's counts win: someone else may have reacted in between, and
      // the optimistic guess only knew about this device.
      setPosts((current) =>
        current.map((p) => (p.id === id ? { ...p, reactions } : p)),
      );
      setProblem(null);
    } catch (error) {
      setPosts(before);
      setProblem(asFeedError(error).message);
    }
  }, [posts]);

  return useMemo(
    () => ({
      posts,
      problem,
      loading,
      loadingMore,
      hasMore,
      loadMore,
      refresh,
      post,
      remove,
      toggleReaction,
      sort,
      setSort,
      following,
      setFollowing,
      castVote,
    }),
    [
      posts,
      problem,
      loading,
      loadingMore,
      hasMore,
      loadMore,
      refresh,
      post,
      remove,
      toggleReaction,
      sort,
      setSort,
      following,
      setFollowing,
      castVote,
    ],
  );
}

/**
 * The optimistic half of a reaction toggle.
 *
 * Exported for its test. The arithmetic is small but it is the part a user
 * sees instantly and would notice being wrong — a count that goes to zero and
 * leaves an empty pill behind, or one that ticks up twice.
 */
export function applyReaction(post: Post, emoji: string, on: boolean): Post {
  const existing = post.reactions.find((r) => r.emoji === emoji);
  const my_reactions = on
    ? [...post.my_reactions, emoji]
    : post.my_reactions.filter((e) => e !== emoji);

  if (!existing) {
    return on
      ? { ...post, my_reactions, reactions: [...post.reactions, { emoji, count: 1 }] }
      : { ...post, my_reactions };
  }

  const count = existing.count + (on ? 1 : -1);
  const reactions =
    count <= 0
      ? post.reactions.filter((r) => r.emoji !== emoji)
      : post.reactions.map((r) => (r.emoji === emoji ? { ...r, count } : r));
  return { ...post, my_reactions, reactions };
}
