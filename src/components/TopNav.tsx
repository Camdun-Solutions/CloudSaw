// Global persistent navigation — Dashboard / Findings / Settings.
//
// Lives at the top-right corner of every authenticated page (anything
// rendered by AppShell). Hidden when the app is locked because the
// UnlockScreen overlays everything else; hidden during onboarding
// because the wizard takes the user through its own ordered flow.
//
// This is route-aware chrome that sits OUTSIDE the page-specific
// header (Home.tsx / Dashboard.tsx / etc. retain their own
// per-page chrome for now — duplicates will be removed in PR #44
// once this nav is verified). Click handlers flip the same
// `route` state AppShell already uses; no new router needed.
//
// Future PRs that touch this component:
//   - PR #42 (lock icon): adds a fourth slot to the right of Settings
//   - PR #48 (Dashboard / Findings overhaul): "Findings" may become
//     its own top-level route once Dashboard is rebuilt as the
//     Welcome page; for now, Findings deep-links into the existing
//     Dashboard component with `initialTab="findings"`.

import { useT } from "@/hooks/useT";

/** Routes the TopNav knows how to navigate to.
 *  Decoupled from the parent `Route` union so this component stays
 *  reusable if route names ever change — the parent translates. */
export type TopNavRoute = "dashboard" | "findings" | "settings";

type Props = {
  /** Currently-active route (so the matching button gets the
   *  visual selected state). The parent App.tsx passes its own
   *  `route` state mapped to one of `TopNavRoute`. Pass `null`
   *  when none of the three is active (e.g. while on Accounts
   *  or Profiles sub-routes). */
  active: TopNavRoute | null;
  onNavigate: (route: TopNavRoute) => void;
};

export default function TopNav({ active, onNavigate }: Props) {
  const t = useT();
  const items: { key: TopNavRoute; label: string }[] = [
    { key: "dashboard", label: t("nav.dashboard") },
    { key: "findings", label: t("nav.findings") },
    { key: "settings", label: t("nav.settings") },
  ];

  return (
    <nav
      aria-label={t("nav.aria_label")}
      // Fixed top-right so the menu stays put regardless of which
      // route is rendered below. `z-30` so it sits above page chrome
      // but BELOW modals (modal overlay is z-50 in `Modal.tsx`).
      className="fixed right-4 top-3 z-30 flex items-center gap-1 rounded-card border border-saw-grey-200 bg-saw-white/95 px-1.5 py-1 shadow-sm backdrop-blur"
      data-testid="top-nav"
    >
      {items.map((item) => {
        const isActive = active === item.key;
        return (
          <button
            key={item.key}
            type="button"
            onClick={() => onNavigate(item.key)}
            aria-current={isActive ? "page" : undefined}
            data-testid={`top-nav-${item.key}`}
            className={
              isActive
                ? "rounded-card bg-saw-red px-3 py-1.5 text-small font-medium text-saw-white transition"
                : "rounded-card px-3 py-1.5 text-small font-medium text-saw-grey-700 transition hover:bg-saw-grey-100 hover:text-saw-grey-900"
            }
          >
            {item.label}
          </button>
        );
      })}
    </nav>
  );
}
