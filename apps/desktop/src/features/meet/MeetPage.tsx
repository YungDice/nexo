import { Suspense, lazy, useCallback, useEffect, useState } from "react";

import { Button } from "../../components/ui/Button";
import { Callout } from "../../components/ui/Feedback";
import {
  acceptAgreement,
  asMeetError,
  leaveMap,
  meetPins,
  myPin,
  setMyPin,
  type MeetMapData,
  type Pin,
} from "../../lib/meet";
import { CharStudio } from "./CharStudio";
import { MeetAgreement } from "./MeetAgreement";
import { MeetCard } from "./MeetCard";
import { Requests } from "./Requests";
import { type CharConfig } from "./NexoChar";

/**
 * Meet&Greet, and the gate in front of it.
 *
 * The map is loaded lazily and nothing else here imports it, so MapLibre, the
 * world data and the character style stay out of the startup bundle
 * entirely — somebody who never opens this tab never pays for any of it.
 *
 * The gate is: agreement, then a character, then a pin. Each step exists
 * because the one after it would otherwise be dishonest — you cannot consent
 * to a map you have not been told about, and you cannot be placed on one
 * without having chosen how you appear.
 */

const MeetMap = lazy(() =>
  import("./MeetMap").then((m) => ({ default: m.MeetMap })),
);

/** Where the map starts before anybody has a pin of their own. */
const DEFAULT_PIN = { lat: 47.4, lon: 8.5 };

type Stage = "loading" | "agreement" | "studio" | "map";

export function MeetPage() {
  const [stage, setStage] = useState<Stage>("loading");
  const [map, setMap] = useState<MeetMapData | null>(null);
  const [mine, setMine] = useState<Pin | null>(null);
  const [config, setConfig] = useState<CharConfig>({});
  const [selected, setSelected] = useState<Pin | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setMap(await meetPins());
    } catch (error) {
      const e = asMeetError(error);
      if (e.kind !== "signed_out") setProblem(e.message);
    }
  }, []);

  // One pass on open: is this person on the map, and what does the map look
  // like? Deliberately not on a timer — see `lib/meet.ts`.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const existing = await myPin();
        if (cancelled) return;
        if (existing) {
          setMine(existing);
          setConfig((existing.char_config ?? {}) as CharConfig);
          setStage("map");
          await refresh();
        } else {
          // Not on the map. Whether that is because they have never agreed or
          // because they left, the agreement is the honest place to restart:
          // accepting again costs one call and re-reads what they are
          // agreeing to.
          setStage("agreement");
        }
      } catch (error) {
        if (cancelled) return;
        const e = asMeetError(error);
        if (e.kind === "signed_out") return;
        setStage("agreement");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refresh]);

  async function agree() {
    setBusy(true);
    try {
      await acceptAgreement();
      setStage("studio");
    } catch (error) {
      setProblem(asMeetError(error).message);
    } finally {
      setBusy(false);
    }
  }

  async function saveCharacter() {
    setBusy(true);
    try {
      // The pin goes in with the character, at the default, because a person
      // on the map without one is not on the map. Moving it is the next thing
      // they can do.
      const stored = await setMyPin({
        ...DEFAULT_PIN,
        char_config: config,
        active: true,
      });
      // What comes back is coarsened, so this is the pin — not what was sent.
      setMine(stored);
      setStage("map");
      await refresh();
    } catch (error) {
      setProblem(asMeetError(error).message);
    } finally {
      setBusy(false);
    }
  }

  async function leave() {
    setBusy(true);
    try {
      await leaveMap();
      setMine(null);
      setStage("agreement");
    } catch (error) {
      setProblem(asMeetError(error).message);
    } finally {
      setBusy(false);
    }
  }

  if (stage === "loading") {
    return <div className="flex-1" />;
  }

  if (stage === "agreement") {
    return (
      <div className="flex min-w-0 flex-1 flex-col">
        <MeetAgreement onAccept={() => void agree()} busy={busy} />
        {problem ? (
          <Callout tone="warning" icon="alert" className="mx-6 mb-4">
            {problem}
          </Callout>
        ) : null}
      </div>
    );
  }

  if (stage === "studio") {
    return (
      <div className="flex min-w-0 flex-1 flex-col p-6">
        <h1 className="text-text-hi font-display mb-1 text-[20px] font-medium">
          Build your character
        </h1>
        <p className="text-text-lo mb-4 text-meta">
          This is how you appear on the map. You can change it whenever you like.
        </p>
        <div className="min-h-0 flex-1">
          <CharStudio
            config={config}
            onChange={setConfig}
            onDone={() => void saveCharacter()}
            busy={busy}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="border-line flex items-center gap-3 border-b px-4 py-2">
        <span className="text-text-lo text-meta">
          {map ? `${map.pins.length} on the map` : "…"}
        </span>
        {map?.stale ? (
          <span className="text-text-lo text-meta">
            &mdash; showing the last map we fetched
          </span>
        ) : null}
        <div className="flex-1" />
        <Button onClick={() => setStage("studio")}>Edit character</Button>
        <Button onClick={() => void leave()} disabled={busy}>
          Leave the map
        </Button>
      </div>

      {problem ? (
        <Callout tone="warning" icon="alert" className="mx-4 mt-3">
          {problem}
        </Callout>
      ) : null}

      <div className="flex min-h-0 flex-1">
        <Suspense fallback={<div className="bg-surface-0 flex-1" />}>
          <MeetMap
            pins={map?.pins ?? []}
            onSelect={(handle) =>
              setSelected(map?.pins.find((p) => p.handle === handle) ?? null)
            }
            className="min-h-0 flex-1"
          />
        </Suspense>

        <aside className="border-line w-[280px] shrink-0 overflow-y-auto border-l p-4">
          <h2 className="text-text-hi mb-3 text-[14px] font-medium">Hellos</h2>
          <Requests />
          {mine ? (
            <p className="text-text-lo mt-6 text-meta">
              Your pin is deliberately imprecise — it says roughly where you
              are, never exactly.
            </p>
          ) : null}
        </aside>
      </div>

      {selected ? (
        <MeetCard
          pin={selected}
          onClose={() => setSelected(null)}
          onBlocked={() => {
            setSelected(null);
            void refresh();
          }}
        />
      ) : null}
    </div>
  );
}
