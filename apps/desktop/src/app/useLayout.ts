import { useEffect, useState } from "react";

/**
 * §7.3 breakpoints.
 *
 * Below 1100px the context panel auto-collapses; below 860px the conversation
 * list becomes an overlay drawer. These are the *only* two width decisions in
 * the app, which is why they are a hook and not a scattering of media queries.
 *
 * They also cover Windows display scaling without a second mechanism: at 150%
 * a 1280px window reports about 853 CSS px, so a scaled-up display and a
 * narrow window take the same path through the layout.
 */
export interface Layout {
  /** The viewport is wide enough for the 280px context panel. */
  canShowContext: boolean;
  /** The viewport is wide enough for the conversation list to sit inline. */
  canShowList: boolean;
}

function read(): Layout {
  if (typeof window === "undefined") return { canShowContext: true, canShowList: true };
  return {
    canShowContext: window.innerWidth >= 1100,
    canShowList: window.innerWidth >= 860,
  };
}

export function useLayout(): Layout {
  const [layout, setLayout] = useState<Layout>(read);

  useEffect(() => {
    const onResize = () => {
      setLayout((current) => {
        const next = read();
        return next.canShowContext === current.canShowContext &&
          next.canShowList === current.canShowList
          ? current
          : next;
      });
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  return layout;
}
