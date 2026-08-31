import type { MyProfile, Visibility, VisibilityField } from "../../lib/feed";
import { cn } from "../../lib/cn";
import { Icon } from "../../components/ui/Icon";
import { SectionHeader } from "../../components/ui/Surface";

/**
 * Per-field profile visibility (G2).
 *
 * Only fields the server can actually honour appear here. Handle, display name,
 * avatar, and Nexo ID are absent on purpose: they are how you are addressed and
 * found, so a switch for them would either break discovery or — far worse — sit
 * in the "private" position while the field stayed visible. A control that
 * cannot be honoured is not a weaker control, it is a false one.
 *
 * The choice is enforced by the server in `profiles::visible_fields`, not by
 * this component deciding what to render. That distinction is the whole feature:
 * filtering a full payload on the client would leave the data one devtools
 * panel away.
 */
const FIELDS: {
  field: VisibilityField;
  label: string;
  hint: string;
}[] = [
  {
    field: "bio",
    label: "Bio",
    hint: "The text under your name.",
  },
  {
    field: "location",
    label: "Location",
    hint: "Starts private. Nexo never asks Windows where you are — this is whatever you typed.",
  },
  {
    field: "links",
    label: "Links",
    hint: "The links on your profile.",
  },
  {
    field: "join_date",
    label: "Join date",
    hint: "When you created this account.",
  },
];

const CHOICES: { value: Visibility; label: string; description: string }[] = [
  { value: "public", label: "Everyone", description: "Anyone signed in to Nexo." },
  {
    value: "contacts",
    label: "Contacts",
    description: "People you have a conversation with.",
  },
  { value: "private", label: "Only me", description: "Nobody else." },
];

export function VisibilityControls({
  profile,
  onChange,
}: {
  profile: MyProfile;
  onChange: (field: VisibilityField, value: Visibility) => void;
}) {
  return (
    <section className="flex flex-col gap-3">
      <SectionHeader>Who can see what</SectionHeader>
      <p className="text-text-mid max-w-[70ch] text-body leading-relaxed">
        Your profile is stored unencrypted on the server, so these settings
        control who it is <em>shown</em> to — not whether the server can read it.
        It can. Your messages are the part nobody but you and the people in them
        can read.
      </p>

      <ul className="rounded-panel divide-y divide-[var(--hairline)] border border-line bg-fill">
        {FIELDS.map(({ field, label, hint }) => (
          <li key={field} className="flex flex-wrap items-center gap-3 p-3.5">
            <div className="min-w-[180px] flex-1">
              <span className="text-text-hi block text-body font-medium">{label}</span>
              <span className="text-text-lo block text-meta leading-relaxed">{hint}</span>
            </div>
            <div
              role="radiogroup"
              aria-label={`Who can see your ${label.toLowerCase()}`}
              className="bg-surface-3 rounded-control flex shrink-0 gap-0.5 p-0.5"
            >
              {CHOICES.map((choice) => {
                const active = profile.visibility[field] === choice.value;
                return (
                  <button
                    key={choice.value}
                    type="button"
                    role="radio"
                    aria-checked={active}
                    title={choice.description}
                    onClick={() => onChange(field, choice.value)}
                    className={cn(
                      "rounded-control px-2.5 py-1.5 text-meta transition-colors duration-[var(--motion-fast)]",
                      active
                        ? "bg-accent/18 text-accent-soft"
                        : "text-text-mid hover:bg-fill-hover",
                    )}
                  >
                    {choice.label}
                  </button>
                );
              })}
            </div>
          </li>
        ))}
      </ul>

      <p className="text-text-lo flex items-start gap-2 px-1 text-meta leading-relaxed">
        <Icon name="shield" size={14} className="mt-0.5 shrink-0" />
        Posts are always public — they are written to be read by strangers, so
        there is nothing here to hide them behind.
      </p>
    </section>
  );
}
