import { useMemo, useState } from "react";
import { useApp } from "../../app/store";
import { useConversations } from "../../app/useConversations";
import { useLayout } from "../../app/useLayout";
import type { Conversation, Message } from "../../lib/types";
import {
  acknowledgeKeyChange,
  asConversationError,
  startConversation,
  startGroup,
} from "../../lib/conversations";
import { Button } from "../../components/ui/Button";
import { Field } from "../../components/ui/Controls";
import { Callout, EmptyState } from "../../components/ui/Feedback";
import { Icon } from "../../components/ui/Icon";
import { Panel } from "../../components/ui/Surface";
import { Composer } from "./Composer";
import { ContextPanel } from "./ContextPanel";
import { ConversationList } from "./ConversationList";
import { MessageList } from "./MessageList";

/**
 * Messages (§7.3): rail, list, chat, context panel.
 *
 * The two width rules live here and nowhere else. Below 1100px the context
 * panel collapses and its content is reachable from the header button; below
 * 860px the conversation list becomes an overlay drawer over the chat.
 */
export function MessagesPage({ now }: { now: Date }) {
  const activeId = useApp((s) => s.activeConversationId);
  const contextOpen = useApp((s) => s.contextPanelOpen);
  const drawerOpen = useApp((s) => s.listDrawerOpen);
  const setDrawer = useApp((s) => s.setListDrawer);
  const toggleContext = useApp((s) => s.toggleContextPanel);
  // Open, not toggle: the banner's button means "show me the safety number",
  // and toggling would hide it for anyone who already had the panel open.
  const openContextPanel = () => {
    if (!contextOpen) toggleContext();
  };
  const open = useApp((s) => s.openConversation);
  const overrides = useApp((s) => s.conversationOverrides);
  const layout = useLayout();

  const live = useConversations(activeId || undefined);
  const [starting, setStarting] = useState(false);

  const base = live.conversations.find((c) => c.id === activeId);
  const conversation = base
    ? { ...base, ...overrides[base.id], safetyDigits: live.safety ?? "" }
    : undefined;
  const showContext = contextOpen && layout.canShowContext;

  // The last message per conversation, for the list rows. Only the active
  // conversation's history is loaded, so every other row falls back to the
  // preview the core already computed.
  const lastMessages = useMemo(() => {
    const map: Record<string, Message | undefined> = {};
    for (const c of live.conversations) {
      map[c.id] =
        c.id === activeId ? live.messages[live.messages.length - 1] : undefined;
    }
    return map;
  }, [live.conversations, live.messages, activeId]);

  const list = (
    <ConversationList
      now={now}
      conversations={live.conversations}
      lastMessages={lastMessages}
      onStart={() => setStarting(true)}
      onRemoved={() => void live.refresh()}
    />
  );

  return (
    <div className="relative flex min-h-0 flex-1">
      {layout.canShowList ? (
        list
      ) : drawerOpen ? (
        <>
          {/* z-index contract, §7.3: overlays sit at 200. */}
          <div
            className="absolute inset-0 bg-scrim"
            style={{ zIndex: 200 }}
            onClick={() => setDrawer(false)}
            aria-hidden="true"
          />
          <div className="absolute inset-y-0 left-0 flex" style={{ zIndex: 200 }}>
            {list}
          </div>
        </>
      ) : null}

      {starting ? (
        <StartConversation
          onCancel={() => setStarting(false)}
          onStarted={(id) => {
            setStarting(false);
            open(id);
            void live.refresh();
          }}
        />
      ) : conversation ? (
        <ChatPane
          conversation={conversation}
          messages={live.messages}
          problem={live.problem}
          now={now}
          onSend={live.send}
          onSendFile={live.sendFile}
          onCompare={openContextPanel}
          onDismissKeyChange={async () => {
            await acknowledgeKeyChange(conversation.id);
            await live.refresh();
          }}
        />
      ) : (
        <Panel tone="content" edge={false} className="flex flex-1 items-center justify-center">
          <EmptyState
            icon="messages"
            title={live.loading ? "Loading" : "No conversation selected"}
            body={
              live.loading
                ? "Reading your local history."
                : "Pick a conversation on the left, or start a new one."
            }
          />
        </Panel>
      )}

      {showContext && conversation && !starting ? (
        <ContextPanel conversation={conversation} now={now} onRefresh={live.refresh} />
      ) : null}
    </div>
  );
}

/**
 * Starting a conversation needs a handle and nothing else.
 *
 * Discovery is by handle only — no phone number is collected anywhere
 * (docs/PLAN.md) — so this is the whole flow.
 */
function StartConversation({
  onCancel,
  onStarted,
}: {
  onCancel: () => void;
  onStarted: (id: string) => void;
}) {
  const [handle, setHandle] = useState("");
  const [title, setTitle] = useState("");
  // Everyone added so far, in order. One handle is a DM; more is a group, and
  // the server decides which from the member count rather than from a toggle
  // the user has to understand.
  const [invited, setInvited] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pending = handle.trim().toLowerCase();
  const everyone = pending && !invited.includes(pending) ? [...invited, pending] : invited;
  const isGroup = everyone.length > 1;

  function addPending() {
    if (!pending || invited.includes(pending)) return;
    setInvited((current) => [...current, pending]);
    setHandle("");
    setError(null);
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (everyone.length === 0 || busy) return;
    setBusy(true);
    setError(null);
    try {
      onStarted(
        everyone.length === 1
          ? await startConversation(everyone[0]!)
          : await startGroup(everyone, title.trim()),
      );
    } catch (raw) {
      const e = asConversationError(raw);
      setError(
        e.kind === "rejected"
          ? "One of those handles has no key package available. Everyone needs to have opened Nexo at least once."
          : e.message,
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel tone="content" edge={false} className="flex flex-1 items-center justify-center p-8">
      <form onSubmit={submit} className="w-full max-w-[340px]">
        <h2 className="text-text-hi font-display text-[19px] font-medium">
          {isGroup ? "Start a group" : "Start a conversation"}
        </h2>
        <p className="text-text-lo mt-1.5 text-meta">
          By handle. Nexo collects no phone numbers, so this is the only way to
          find someone. Add a second person to make it a group.
        </p>

        {invited.length > 0 ? (
          <ul className="mt-3 flex flex-wrap gap-1.5">
            {invited.map((who) => (
              <li key={who}>
                <button
                  type="button"
                  onClick={() => setInvited((c) => c.filter((h) => h !== who))}
                  className="rounded-control bg-fill text-text-hi hover:bg-fill-hover flex items-center gap-1.5 border border-line px-2 py-1 text-[11px]"
                  aria-label={`Remove ${who}`}
                >
                  @{who}
                  <Icon name="close" size={11} />
                </button>
              </li>
            ))}
          </ul>
        ) : null}

        <Field
          label="Handle"
          className="mt-4"
          value={handle}
          spellCheck={false}
          autoCapitalize="none"
          autoCorrect="off"
          placeholder="alice"
          onChange={(e) => setHandle(e.target.value.toLowerCase())}
          onKeyDown={(e) => {
            // Enter adds another rather than submitting, so building a group
            // never means accidentally starting a DM with the first name.
            if (e.key === "Enter" && pending) {
              e.preventDefault();
              addPending();
            }
          }}
        />

        {pending ? (
          <button
            type="button"
            onClick={addPending}
            className="text-accent-soft mt-2 text-[11px] underline decoration-line-strong underline-offset-2"
          >
            Add another person
          </button>
        ) : null}

        {isGroup ? (
          <Field
            label="Group name"
            className="mt-3"
            value={title}
            placeholder="Weekend plans"
            onChange={(e) => setTitle(e.target.value)}
          />
        ) : null}

        {error ? (
          <Callout tone="danger" icon="alert" className="mt-3">
            {error}
          </Callout>
        ) : null}

        <div className="mt-4 flex gap-2">
          <Button type="submit" variant="primary" disabled={everyone.length === 0 || busy}>
            {busy ? "Starting…" : isGroup ? `Start group (${everyone.length})` : "Start"}
          </Button>
          <Button type="button" onClick={onCancel}>
            Cancel
          </Button>
        </div>
      </form>
    </Panel>
  );
}

function ChatPane({
  conversation,
  messages,
  problem,
  now,
  onSend,
  onSendFile,
  onCompare,
  onDismissKeyChange,
}: {
  conversation: Conversation;
  messages: Message[];
  problem: string | null;
  now: Date;
  onSend: (body: string) => Promise<void>;
  onSendFile: (path: string, body?: string) => Promise<void>;
  /// Opens the details panel, where the safety number is.
  onCompare: () => void;
  /// Clears the warning without claiming anything was verified.
  onDismissKeyChange: () => Promise<void>;
}) {
  return (
    <Panel tone="content" edge={false} className="flex min-w-0 flex-1 flex-col">
      {/* Not dismissable by ignoring it, and not by restarting: the flag lives
          in the encrypted store, so closing the window does not clear it.
          THREAT-MODEL 4 names a key-substituting server as the adversary safety
          numbers exist to catch, and this is the only moment a user is told
          there is something to compare. */}
      {conversation.keyChanged ? (
        <Callout tone="danger" icon="alert" className="mx-3 mt-3">
          <p className="font-medium">The safety number here has changed.</p>
          <p className="mt-1 leading-relaxed">
            Somebody in this conversation is using a new key. That happens when
            they reinstall Nexo or move to another machine — and it is also what
            an attacker substituting a key would look like. Nexo cannot tell
            those apart. Compare the safety number with them before sending
            anything you would not want a stranger to read.
          </p>
          <div className="mt-2.5 flex gap-2">
            <Button variant="primary" onClick={onCompare}>
              Compare safety numbers
            </Button>
            <Button onClick={() => void onDismissKeyChange()}>Dismiss</Button>
          </div>
        </Callout>
      ) : null}

      {problem ? (
        <Callout tone="warning" icon="alert" className="mx-3 mt-3">
          {problem}
        </Callout>
      ) : null}

      {/* N5: in an empty conversation the composer sits with the invitation
          rather than pinned to the bottom of an empty page. There is nothing
          above it to anchor to yet, and a lone box at the foot of a blank
          panel reads as the end of something instead of the start. It moves
          to its usual place as soon as there is a history to sit under. */}
      {messages.length === 0 ? (
        <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 px-4">
          <EmptyState
            icon="messages"
            title={`Say something to ${conversation.title}`}
            body="Messages here are end-to-end encrypted. Only the people in this conversation can read them."
          />
          <div className="w-full max-w-[560px]">
            <Composer
              onSend={(body, attachment) => {
                if (attachment) void onSendFile(attachment.path, body);
                else void onSend(body);
              }}
              conversationTitle={conversation.title}
            />
          </div>
        </div>
      ) : (
        <>
          <MessageList messages={messages} now={now} conversation={conversation} />
          <Composer
            onSend={(body, attachment) => {
              if (attachment) void onSendFile(attachment.path, body);
              else void onSend(body);
            }}
            conversationTitle={conversation.title}
          />
        </>
      )}
    </Panel>
  );
}
