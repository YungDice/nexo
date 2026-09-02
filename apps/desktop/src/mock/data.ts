/**
 * Mock data for M1.
 *
 * The milestone is "all three pages shelled out with mock data. No network."
 * — so this file is the whole data layer, and every surface reads from it.
 * Two rules kept it honest:
 *
 * 1. It is built from an explicit `now`, so "9:45 am" and "yesterday" are
 *    stable in tests and alive in the app.
 * 2. The content is plausible product content. No lorem ipsum, and nothing
 *    that claims a security property the app does not have (rule 5).
 */

import type {
  Conversation,
  Message,
  Person,
  Post,
  SharedLink,
} from "../lib/types";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export interface MockData {
  me: Person;
  people: Person[];
  conversations: Conversation[];
  messages: Message[];
  posts: Post[];
  links: SharedLink[];
  /** Numeric device fingerprint for the Security tab, 60 digits (§4.1). */
  deviceFingerprint: string;
}

function ago(now: Date, ms: number): Date {
  return new Date(now.getTime() - ms);
}

export function buildMockData(now: Date): MockData {
  const me: Person = {
    id: "u-me",
    handle: "carter",
    displayName: "Carter Donin",
    userId: 100_248,
    bio: "Designing the parts of software people actually touch. Currently on interface systems for small teams.",
    location: "Zürich",
    links: ["dice.fit/carter"],
    joinedAt: new Date(now.getFullYear(), 2, 14),
    presence: "online",
    lastSeen: now,
  };

  const people: Person[] = [
    me,
    {
      id: "u-abram",
      handle: "abram",
      displayName: "Abram Culhane",
      userId: 100_311,
      bio: "Backend, mostly Rust. Ask me about database migrations at your own risk.",
      location: "Rotterdam",
      links: ["dice.fit/abram"],
      joinedAt: new Date(now.getFullYear(), 3, 2),
      presence: "online",
      lastSeen: now,
    },
    {
      id: "u-chance",
      handle: "chance",
      displayName: "Chance Baptiste",
      userId: 100_402,
      bio: "Product. Writes the questions nobody wants asked in week one.",
      location: "Lisbon",
      links: [],
      joinedAt: new Date(now.getFullYear(), 3, 19),
      presence: "away",
      lastSeen: ago(now, 22 * MINUTE),
    },
    {
      id: "u-desirae",
      handle: "desirae",
      displayName: "Desirae Dias",
      userId: 100_517,
      bio: "Motion and interaction. Half of my job is deleting animations.",
      location: "Berlin",
      links: ["dice.fit/desirae"],
      joinedAt: new Date(now.getFullYear(), 4, 6),
      presence: "offline",
      lastSeen: ago(now, 9 * HOUR),
    },
    {
      id: "u-nolan",
      handle: "nolan",
      displayName: "Nolan Mango",
      userId: 100_688,
      bio: "Type, grids, and the argument for smaller interfaces.",
      location: "Manchester",
      links: [],
      joinedAt: new Date(now.getFullYear(), 4, 27),
      presence: "online",
      lastSeen: now,
    },
    {
      id: "u-skylar",
      handle: "skylar",
      displayName: "Skylar Rosser",
      userId: 100_741,
      bio: "Security engineering. Sceptical by trade, not by temperament.",
      location: "Tallinn",
      links: ["dice.fit/skylar"],
      joinedAt: new Date(now.getFullYear(), 5, 11),
      presence: "offline",
      lastSeen: ago(now, 2 * DAY),
    },
    {
      id: "u-mira",
      handle: "mira",
      displayName: "Mira Delacroix",
      userId: 100_803,
      bio: "Writes the words in the product, including this one.",
      location: "Montréal",
      links: [],
      joinedAt: new Date(now.getFullYear(), 5, 30),
      presence: "away",
      lastSeen: ago(now, 3 * HOUR),
    },
  ];

  const conversations: Conversation[] = [
    {
      id: "c-design",
      kind: "group",
      title: "Design team",
      memberIds: ["u-me", "u-nolan", "u-desirae", "u-mira", "u-chance"],
      unread: 0,
      verified: false,
      safetyDigits:
        "418290734615082973461508297346150829734615082973461508418290",
      muted: false,
    },
    {
      id: "c-abram",
      kind: "dm",
      title: "Abram Culhane",
      memberIds: ["u-me", "u-abram"],
      unread: 4,
      verified: true,
      safetyDigits:
        "902371845620193847562019384756201938475620193847562019902371",
      muted: false,
    },
    {
      id: "c-chance",
      kind: "dm",
      title: "Chance Baptiste",
      memberIds: ["u-me", "u-chance"],
      unread: 1,
      verified: false,
      safetyDigits:
        "553019284736550192847365501928473655019284736550192847553019",
      muted: false,
    },
    {
      id: "c-desirae",
      kind: "dm",
      title: "Desirae Dias",
      memberIds: ["u-me", "u-desirae"],
      unread: 0,
      verified: true,
      safetyDigits:
        "117482930561174829305611748293056117482930561174829305117482",
      muted: true,
    },
    {
      id: "c-nolan",
      kind: "dm",
      title: "Nolan Mango",
      memberIds: ["u-me", "u-nolan"],
      unread: 0,
      verified: false,
      safetyDigits:
        "640182937455640182937455640182937455640182937455640182640182",
      muted: false,
    },
    {
      id: "c-skylar",
      kind: "dm",
      title: "Skylar Rosser",
      memberIds: ["u-me", "u-skylar"],
      unread: 0,
      verified: true,
      safetyDigits:
        "308561947230856194723085619472308561947230856194723085308561",
      muted: false,
    },
  ];

  const messages: Message[] = [
    // Design team — the conversation the app opens on.
    {
      id: "m-1",
      conversationId: "c-design",
      authorId: "u-nolan",
      body: "Reworked the conversation list overnight. The unread pill was fighting the timestamp at 150% scaling, so the timestamp moves up and the pill sits under it.",
      at: ago(now, 27 * HOUR),
      state: "read",
      attachments: [
        {
          id: "i-list-a",
          name: "list-100.png",
          size: 486_000,
          mime: "image/png",
          kind: "image",
        },
        {
          id: "i-list-b",
          name: "list-125.png",
          size: 502_400,
          mime: "image/png",
          kind: "image",
        },
        {
          id: "i-list-c",
          name: "list-150.png",
          size: 531_200,
          mime: "image/png",
          kind: "image",
        },
      ],
    },
    {
      id: "m-2",
      conversationId: "c-design",
      authorId: "u-me",
      body: "That reads better. Does it hold at 720px wide?",
      at: ago(now, 26 * HOUR),
      state: "read",
    },
    {
      id: "m-3",
      conversationId: "c-design",
      authorId: "u-nolan",
      body: "It does. Below 860 the list becomes a drawer anyway, so the row gets its full width back.",
      at: ago(now, 25 * HOUR),
      state: "read",
    },
    {
      id: "m-4",
      conversationId: "c-design",
      authorId: "u-desirae",
      body: "Sending the panel spec — blur radius, edge colour, and the opaque fallback for machines that can't afford it.",
      at: ago(now, 5 * HOUR),
      state: "read",
      attachments: [
        {
          id: "f-panels",
          name: "panels.sketch",
          size: 204_800,
          mime: "application/octet-stream",
          kind: "file",
        },
        {
          id: "f-fallback",
          name: "opaque-fallback.pdf",
          size: 1_268_000,
          mime: "application/pdf",
          kind: "file",
        },
      ],
    },
    {
      id: "m-5",
      conversationId: "c-design",
      authorId: "u-mira",
      body: "One copy note before this ships: the feed can't say \"encrypted\". It isn't. The composer needs a line that says posts are public.",
      at: ago(now, 4 * HOUR),
      state: "read",
    },
    {
      id: "m-6",
      conversationId: "c-design",
      authorId: "u-me",
      body: "Agreed, and it goes in Settings too. Being vague about that is worse than not shipping the feed.",
      at: ago(now, 4 * HOUR + 40 * MINUTE),
      state: "read",
    },
    {
      id: "m-7",
      conversationId: "c-design",
      authorId: "u-nolan",
      body: "Here's the dark pass on the message pane, with the composer at the new height.",
      at: ago(now, 95 * MINUTE),
      state: "read",
      attachments: [
        {
          id: "i-pane-a",
          name: "pane-dark.png",
          size: 742_000,
          mime: "image/png",
          kind: "image",
        },
        {
          id: "i-pane-b",
          name: "composer.png",
          size: 318_500,
          mime: "image/png",
          kind: "image",
        },
      ],
      preview: {
        url: "https://dice.fit/nolan/panes",
        title: "Message pane, dark",
        description:
          "Bubble grouping, day dividers and the composer at 56px. Timestamps sit under the last bubble in a run rather than on every one.",
        source: "dice.fit",
      },
    },
    {
      id: "m-8",
      conversationId: "c-design",
      authorId: "u-desirae",
      body: "",
      at: ago(now, 80 * MINUTE),
      state: "read",
      undecryptable: true,
    },
    {
      id: "m-9",
      conversationId: "c-design",
      authorId: "u-me",
      body: "That's the one. Ship it once the focus rings are on the composer buttons.",
      at: ago(now, 12 * MINUTE),
      state: "read",
    },
    {
      id: "m-10",
      conversationId: "c-design",
      authorId: "u-mira",
      body: "Rings are in. I'll take the empty states next — they should invite the first message, not apologise for the silence.",
      at: ago(now, 3 * MINUTE),
      state: "delivered",
    },

    // Abram — unread, and the offline-queue case.
    {
      id: "m-11",
      conversationId: "c-abram",
      authorId: "u-abram",
      body: "The delivery service orders commits per conversation now. Two clients racing at the same epoch: first writer wins, the loser gets StaleEpoch and rebuilds.",
      at: ago(now, 52 * MINUTE),
      state: "read",
    },
    {
      id: "m-12",
      conversationId: "c-abram",
      authorId: "u-me",
      body: "Good. Does the loser lose the message, or does it resend after the rebuild?",
      at: ago(now, 48 * MINUTE),
      state: "read",
    },
    {
      id: "m-13",
      conversationId: "c-abram",
      authorId: "u-abram",
      body: "Resends. It's the same queue the offline case uses, so there's one retry path instead of two.",
      at: ago(now, 41 * MINUTE),
      state: "read",
    },
    {
      id: "m-14",
      conversationId: "c-abram",
      authorId: "u-me",
      body: "I'll write it up in the plan under risk 4.",
      at: ago(now, 45 * MINUTE),
      state: "sending",
    },

    // Chance
    {
      id: "m-15",
      conversationId: "c-chance",
      authorId: "u-chance",
      body: 'Are we still calling the numeric ID an ID everywhere, or does it become a "Nexo number" in the profile?',
      at: ago(now, 3 * HOUR),
      state: "read",
    },

    // Desirae
    {
      id: "m-16",
      conversationId: "c-desirae",
      authorId: "u-desirae",
      body: "Motion spec is one line long: messages rise eight pixels over 180ms, everything else is 120.",
      at: ago(now, 20 * HOUR),
      state: "read",
    },
    {
      id: "m-17",
      conversationId: "c-desirae",
      authorId: "u-me",
      body: "That's the whole spec and it should stay that short.",
      at: ago(now, 19 * HOUR),
      state: "read",
    },

    // Nolan
    {
      id: "m-18",
      conversationId: "c-nolan",
      authorId: "u-me",
      body: "Sent the grid over. Same 4px base, 64px rail.",
      at: ago(now, 2 * DAY),
      state: "read",
    },

    // Skylar — the safety-number thread.
    {
      id: "m-19",
      conversationId: "c-skylar",
      authorId: "u-skylar",
      body: "Compared safety numbers on the call. They match, so I've marked this one verified on my side.",
      at: ago(now, 4 * DAY),
      state: "read",
    },
    {
      id: "m-20",
      conversationId: "c-skylar",
      authorId: "u-me",
      body: "Marked here too.",
      at: ago(now, 4 * DAY),
      state: "read",
    },
  ];

  const posts: Post[] = [
    {
      id: "p-0",
      authorId: "u-me",
      body: "Shipped the shell today: three destinations, one design system, and mock data behind all of it. Nothing talks to a server yet, which is the point — the layout has to survive 150% scaling before it earns a network.",
      media: ["media-shell-1"],
      at: ago(now, 2 * HOUR),
      reactions: [{ emoji: "\u{1F44D}", count: 9, mine: false }],
      comments: 1,
    },
    {
      id: "p-1",
      authorId: "u-skylar",
      body: "A messenger that hides what it can't protect is worse than one that says so plainly. Metadata — who talks to whom, and when — is visible to whoever runs the server. That's true of Nexo too, and it's in Settings in those words.",
      media: [],
      at: ago(now, 35 * MINUTE),
      reactions: [
        { emoji: "👍", count: 14, mine: true },
        { emoji: "🔒", count: 6, mine: false },
      ],
      comments: 3,
    },
    {
      id: "p-2",
      authorId: "u-nolan",
      body: "Three weeks on the conversation list and the change that mattered was removing things: no avatar ring, no second line of metadata, no hover reveal. Density comes from restraint, not from smaller type.",
      media: ["media-list-1", "media-list-2"],
      at: ago(now, 4 * HOUR),
      reactions: [{ emoji: "👍", count: 22, mine: false }],
      comments: 7,
    },
    {
      id: "p-3",
      authorId: "u-mira",
      body: "Error copy rule we're keeping: say what happened and what happens next. \"Can't reach the server. Your message will send when you're back online.\" Never an apology, never an exclamation mark.",
      media: [],
      at: ago(now, 9 * HOUR),
      reactions: [
        { emoji: "👍", count: 31, mine: true },
        { emoji: "✍️", count: 4, mine: false },
      ],
      comments: 12,
    },
    {
      id: "p-4",
      authorId: "u-desirae",
      body: "Reduced motion isn't a per-component checkbox. It's one media query at the root that flattens every duration in the system to zero, and then you can't forget it.",
      media: ["media-motion-1"],
      at: ago(now, 26 * HOUR),
      reactions: [{ emoji: "👍", count: 18, mine: false }],
      comments: 2,
    },
    {
      id: "p-5",
      authorId: "u-abram",
      body: "Spent the morning proving that a member added at epoch 42 cannot read epoch 41. The test is nine lines and it is the most valuable thing I wrote this week.",
      media: [],
      at: ago(now, 2 * DAY),
      reactions: [
        { emoji: "👍", count: 44, mine: true },
        { emoji: "🔒", count: 11, mine: false },
      ],
      comments: 5,
    },
  ];

  const links: SharedLink[] = [
    {
      id: "l-1",
      url: "https://dice.fit/nolan/panes",
      title: "Message pane, dark",
      source: "dice.fit",
      at: ago(now, 95 * MINUTE),
    },
    {
      id: "l-2",
      url: "https://www.rfc-editor.org/rfc/rfc9420.html",
      title: "RFC 9420 — Messaging Layer Security",
      source: "rfc-editor.org",
      at: ago(now, 2 * DAY),
    },
    {
      id: "l-3",
      url: "https://dice.fit/desirae/motion",
      title: "Motion spec, one page",
      source: "dice.fit",
      at: ago(now, 6 * DAY),
    },
  ];

  return {
    me,
    people,
    conversations,
    messages,
    posts,
    links,
    deviceFingerprint:
      "729140385620617394852073619408572639184057263918405726729140",
  };
}

/** Built once at module load so the app has a consistent sense of "now". */
export const mock: MockData = buildMockData(new Date());

export function personById(data: MockData, id: string): Person {
  const found = data.people.find((p) => p.id === id);
  if (!found) throw new Error(`unknown person: ${id}`);
  return found;
}

export function messagesFor(data: MockData, conversationId: string): Message[] {
  return data.messages
    .filter((m) => m.conversationId === conversationId)
    .sort((a, b) => a.at.getTime() - b.at.getTime());
}

/** The preview line in a conversation row, and the sort key for the list. */
export function lastMessage(
  data: MockData,
  conversationId: string,
): Message | undefined {
  const all = messagesFor(data, conversationId);
  return all[all.length - 1];
}

/** Every attachment ever seen in a conversation, newest first (§6.1). */
export function sharedAttachments(data: MockData, conversationId: string) {
  return messagesFor(data, conversationId)
    .flatMap((m) =>
      (m.attachments ?? []).map((a) => ({ attachment: a, at: m.at })),
    )
    .reverse();
}
