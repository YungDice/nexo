# Component library (G4)

The documented component library the source prompt asked for. The library
*is* `apps/desktop/src/components/` — this document is its catalogue and its
rules. No Storybook: the library is fourteen small components and the app
itself exercises every one of them on real data, which is the gallery a
Storybook would have to fake. Revisit that trade-off when a component gains a
state the app cannot reach, because that is the moment it can silently break.

Everything here builds on `packages/design-tokens` (G3): components spend
*named* colours, radii and durations — `--text-hi`, `rounded-control`,
`var(--motion-fast)` — and never raw values. That is what makes light mode,
dark mode, and the frosted/opaque toggle work everywhere at once, and it is
rule one below.

## Rules, before the catalogue

1. **Tokens only.** A component that writes a hex colour or a millisecond
   number has left the design system; both themes and the glass toggle stop
   being guaranteed to hold. The tokens live in
   `apps/desktop/src/styles/tokens.css`, and `packages/design-tokens` derives
   the Android-readable `tokens.json` from them — a test fails if they drift.
2. **No component touches `backdrop-filter`.** Surfaces ask `Panel` for a
   tone (`glass-0`–`glass-3`); the Settings toggle and the `@supports`
   fallback swap all of them at once (plan risk 9). The *window's* translucency
   is not a component's business either: it is an OS effect (`set_backdrop` in
   Rust) reported back to the root element as `data-backdrop`, and the
   stylesheet answers it. What a component must not do is paint an opaque
   background across a whole pane — that is the one thing that takes both
   effects away at once.
3. **Icon-only controls carry a required `label`.** §7.4 makes screen-reader
   labels part of the quality floor, and a required prop is the only form of
   that rule that survives a deadline.
4. **Feedback is honest by construction.** `Callout` is the one way the UI
   states a security boundary, so the wording cannot drift between screens;
   `Skeleton` shims at the size of what is coming; there are no circular
   spinners.
5. **Two motion curves, and they are not interchangeable.**
   `--ease-out` is near-exponential and belongs to things *arriving*: a
   message, a card, a panel opening. `--ease-state` is symmetric and belongs
   to anything switching between two states — hover, focus, selection, a
   colour. Using the entrance curve for a hover is what makes a short
   transition read as a jump: almost all of the distance is covered before
   the eye registers it started. The rule in practice: `animation:` takes
   `--ease-out`, `transition-*` takes `--ease-state`.
6. **Animate `transform` and `opacity`, never `left`, `width` or
   `background-position`.** The first two are handed to the compositor; the
   rest ask the browser to lay out or repaint on every frame. Both look
   identical at rest, which is why the slow one keeps getting written — the
   difference only shows up as judder while the app is busy.
7. **New components start here.** A second use is the bar for extraction from
   a feature into `components/ui`; a first use is not.

## `components/ui`

### Surface.tsx

| Export | What it is |
|---|---|
| `Panel` | The glass pane (§7.1). `tone`: `rail` \| `list` \| `content` \| `raised` → `glass-0`–`3`; `edge` adds the lit top edge. The only place translucency exists. |
| `SectionHeader` | Display-face section heading, used sparingly so it stays a signal. |
| `GroupLabel` | Small-caps label for groups within a pane. |
| `Divider` | Hairline rule matching the pane's border colour. |

### Button.tsx

| Export | What it is |
|---|---|
| `Button` | `variant`: `primary` (the accent — one per view, it means "this one") \| `secondary` (default) \| `ghost` \| `danger`; optional leading `icon`. Extends the native button, so `disabled`, `type`, `onClick` are the platform's. |
| `IconButton` | Icon-only button. `name` picks the glyph, `label` is **required** (rule 3), `active` marks a pressed toggle state. |

### Controls.tsx

| Export | What it is |
|---|---|
| `Field` | Single-line input with the label above (never floating). `hint`, `error`, optional `icon`, `hideLabel` for chrome search boxes (label stays for screen readers). |
| `TextArea` | The multi-line `Field`. |
| `Toggle` | Switch with `label` and optional `description`; the whole row is the hit target. |
| `Tabs` | `role="tablist"` with the accent underline on the active tab. Generic over the tab-id union, so a typo in an id is a type error. |
| `FactRow` | A read-only icon + label + value row — join date, numeric ID, epoch. |

### Feedback.tsx

| Export | What it is |
|---|---|
| `Callout` | The honesty banner (rule 5 of the brief, §4.4). `tone`: `neutral` \| `warning` \| `danger`. Security boundaries are stated with this and nothing else. |
| `EmptyState` | §6.2: icon, one line of what this place is for, one action. Invites, never apologises. |
| `Pill` | Status chip. `tone`: `neutral` \| `accent` \| `success` \| `warning` \| `danger`. |
| `Skeleton` | Shimmer at the size of the thing that is coming (rule 4). |

### Icon.tsx

| Export | What it is |
|---|---|
| `Icon` | Every glyph in the app: one inline-SVG set, 1.5px stroke, `currentColor`. `name` is the `IconName` union — adding a glyph means adding it here, so the app cannot half-adopt a second icon style. |
| `iconNames` | The full list, for tooling and tests. |

### Avatar.tsx

`Avatar` — a generated identicon: gradient plus initials, both derived from a
stable `seed`, with an optional `presence` dot and an optional real picture
(`imageUrl`, already a WebView-loadable URL). Generated rather than fetched
because the CSP has no remote image host to fetch from (§4.5).

### RemoteImage.tsx

`RemoteImage` — an image in private object storage, rendered from its S3
key. Asks Rust for a presigned 60-minute GET, memoises per key for the
process lifetime, never persists URLs (a presigned URL is a bearer
credential). A generated field stands in while loading and stays on failure,
so a dead object cannot collapse the layout.

## `components/chrome`

The frameless-window furniture (§7.3): not reusable widgets, but documented
here because everything else lives inside them.

| Export | What it is |
|---|---|
| `TopBar` | The one chrome row: wordmark, page cell, caption buttons. Carries the drag region and the three Windows caption buttons the capability file allows. |
| `PageTitleCell` | Title + actions cell rendered into the `TopBar` per route. |
| `IconRail` | The 64px destination rail. Takes the live unread total for the Messages badge. |

## Feature components worth knowing about

Not part of the shared library — they live with their features — but they are
the ones another screen is most likely to want, and each encodes a rule rather
than just a layout:

| Where | What it is |
|---|---|
| `features/settings/ChangePassword` | The change-password form (§6.4). Asks for the current password even though the session is signed in, and says why in its own doc comment. |
| `features/settings/UnlockPin` | Sets and clears the unlock PIN, beside the password and the auto-lock timer it serves. Was in the profile, which split one feature across two menus. |
| `features/settings/PrivacyTable` | The §4.4 honesty table: what is end-to-end encrypted and what is not. |
| `features/auth/LockScreen` | Drawn *instead of* the shell when locked, never over it, so no conversation sits in the DOM underneath. |
| `features/home/HomeChat` | The most recent conversation beside the feed, on Home. Composes the real `MessageList` and `Composer` rather than lookalikes, so message grouping, delivery states and the offline-queue mark cannot drift between the two places they appear. |
| `features/home/Splitter` | The draggable line between the feed and that conversation. Owns no width — it measures the gesture and reports a number, separately for "while dragging" and "done", because persisting on every frame puts a `localStorage` write on every frame. Keyboard-operable, like anything else that changes the layout (§7.4). |
| `features/messages/LinkPreviewCard` | Renders a preview only once one has arrived (no skeleton for something that may never come), and a bare link otherwise. |

## Composition, in one example

```tsx
<Panel tone="content" edge={false}>
  <SectionHeader>Recovery</SectionHeader>
  <Callout tone="warning" icon="alert" title="There is no account recovery.">
    Your identity key exists only on this machine.
  </Callout>
  <Button variant="primary" icon="check" onClick={confirm}>
    I understand
  </Button>
</Panel>
```

Every screen in the app is these pieces over tokens; the features under
`src/features/` add layout and data, not new visual vocabulary.
