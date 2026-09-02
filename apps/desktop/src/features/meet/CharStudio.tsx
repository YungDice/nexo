import { useMemo, useState } from "react";

import { OptionsDescriptor } from "@dicebear/core";

import { Button } from "../../components/ui/Button";
import { NexoChar, VOXEL_ART, type CharConfig } from "./NexoChar";

/**
 * Building a NexoChar.
 *
 * **The controls are generated, not written.** `OptionsDescriptor` reports
 * every option the style accepts, with its type and its allowed values, and
 * this walks that. A DiceBear update that adds a hairstyle adds it to this UI
 * with no code change, and there is no second list to drift out of step with
 * the first.
 *
 * The consequence worth stating: nothing here hardcodes a variant name. If a
 * group below is empty it is because the style stopped offering it, which is
 * information rather than a bug.
 */

/** The option groups, in the order somebody would work through them. */
const PART_ORDER = [
  "top",
  "eyes",
  "eyebrows",
  "mouth",
  "nose",
  "beard",
  "cheeks",
  "glasses",
  "outfit",
] as const;

const COLOR_ORDER = [
  "skin",
  "hair",
  "hat",
  "shirt",
  "jacket",
  "pants",
  "shoes",
  "background",
] as const;

/** A colour swatch set, so a colour field is a palette rather than a text box. */
const SWATCHES = [
  "b6e3f4",
  "c0aede",
  "d1d4f9",
  "ffd5dc",
  "ffdfbf",
  "f5f5f5",
  "8e8e93",
  "2c2c2e",
  "e8734a",
  "3f8f6b",
  "4a6fa5",
  "b07d3a",
];

type Descriptor = ReturnType<OptionsDescriptor["toJSON"]>;

/** Turn `topVariant` into `top`, `skinColor` into `skin`. */
function groupOf(key: string): string | null {
  if (key.endsWith("Variant")) return key.slice(0, -"Variant".length);
  if (key.endsWith("Color")) return key.slice(0, -"Color".length);
  return null;
}

export function CharStudio({
  config,
  onChange,
  onDone,
  busy,
}: {
  config: CharConfig;
  onChange: (next: CharConfig) => void;
  onDone?: () => void;
  busy?: boolean;
}) {
  const descriptor: Descriptor = useMemo(
    () => new OptionsDescriptor(VOXEL_ART).toJSON(),
    [],
  );

  const [tab, setTab] = useState<"parts" | "colours">("parts");

  // Built from the descriptor rather than from a list here, so the two cannot
  // disagree. The declared order above is a preference; anything the style
  // offers that is not in it still appears, at the end.
  const { parts, colours } = useMemo(() => {
    const parts: { key: string; group: string; values: readonly string[] }[] = [];
    const colours: { key: string; group: string }[] = [];
    for (const [key, field] of Object.entries(descriptor)) {
      const group = groupOf(key);
      if (!group) continue;
      if (field.type === "enum") {
        parts.push({ key, group, values: field.values });
      } else if (field.type === "color") {
        colours.push({ key, group });
      }
    }
    const rank = (order: readonly string[], group: string) => {
      const i = order.indexOf(group);
      return i === -1 ? order.length : i;
    };
    parts.sort((a, b) => rank(PART_ORDER, a.group) - rank(PART_ORDER, b.group));
    colours.sort(
      (a, b) => rank(COLOR_ORDER, a.group) - rank(COLOR_ORDER, b.group),
    );
    return { parts, colours };
  }, [descriptor]);

  const set = (key: string, value: unknown) => {
    // Every option is stored as a single-element list. That is the shape
    // DiceBear treats as "this exactly", rather than "pick from these".
    onChange({ ...config, [key]: [value] });
  };

  const current = (key: string): string | undefined => {
    const value = config[key];
    return Array.isArray(value) ? (value[0] as string) : (value as string);
  };

  return (
    <div className="flex h-full min-h-0 gap-6">
      <div className="flex shrink-0 flex-col items-center gap-3">
        <div className="rounded-panel bg-surface-2 border border-line p-4">
          <NexoChar config={config} size={180} title="Your character" />
        </div>
        {onDone ? (
          <Button variant="primary" onClick={onDone} disabled={busy}>
            {busy ? "Saving…" : "Use this character"}
          </Button>
        ) : null}
      </div>

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="mb-3 flex gap-2">
          <Button
            variant={tab === "parts" ? "primary" : "secondary"}
            onClick={() => setTab("parts")}
          >
            Parts
          </Button>
          <Button
            variant={tab === "colours" ? "primary" : "secondary"}
            onClick={() => setTab("colours")}
          >
            Colours
          </Button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto pr-1">
          {tab === "parts"
            ? parts.map(({ key, group, values }) => (
                <section key={key} className="mb-5">
                  <h3 className="text-text-lo mb-2 text-meta capitalize">{group}</h3>
                  <div className="flex flex-wrap gap-2">
                    {values.map((value) => (
                      <button
                        key={value}
                        type="button"
                        onClick={() => set(key, value)}
                        aria-pressed={current(key) === value}
                        className={`rounded-control border px-2.5 py-1 text-[12px] transition-colors ${
                          current(key) === value
                            ? "border-accent text-text-hi bg-surface-3"
                            : "border-line text-text-lo hover:bg-surface-2"
                        }`}
                      >
                        {value}
                      </button>
                    ))}
                  </div>
                </section>
              ))
            : colours.map(({ key, group }) => (
                <section key={key} className="mb-5">
                  <h3 className="text-text-lo mb-2 text-meta capitalize">{group}</h3>
                  <div className="flex flex-wrap gap-2">
                    {SWATCHES.map((hex) => (
                      <button
                        key={hex}
                        type="button"
                        onClick={() => set(key, hex)}
                        aria-label={`${group} ${hex}`}
                        aria-pressed={current(key) === hex}
                        className={`h-7 w-7 rounded-full border-2 transition-transform ${
                          current(key) === hex
                            ? "border-accent scale-110"
                            : "border-line hover:scale-105"
                        }`}
                        // A generator palette, not a design token: these are
                        // values handed to DiceBear, not colours of the app.
                        style={{ backgroundColor: `#${hex}` }}
                      />
                    ))}
                  </div>
                </section>
              ))}
        </div>
      </div>
    </div>
  );
}
