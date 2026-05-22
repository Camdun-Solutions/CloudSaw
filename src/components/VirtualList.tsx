// Tiny virtualization helper. Renders only the rows currently inside the
// scroll viewport plus a small overscan window, so a finding list of tens
// of thousands stays interactive (Contract 09 §Edge Cases + §Acceptance
// Criteria).
//
// Constraints:
//   - Fixed row height. The findings list uses a uniform row, so we don't
//     need the complexity of variable-height virtualization.
//   - Keyboard scrolling continues to work because the scroll container
//     is a real <div> and selected rows can be focused (the list rows
//     are role="row" with tabIndex 0 in the consuming component).

import { useEffect, useRef, useState, type ReactNode } from "react";

type Props<T> = {
  items: T[];
  rowHeight: number;
  height: number;
  overscan?: number;
  renderRow: (item: T, index: number) => ReactNode;
  /** Aria-label for the outer scroll region. */
  ariaLabel?: string;
  className?: string;
};

export default function VirtualList<T>({
  items,
  rowHeight,
  height,
  overscan = 6,
  renderRow,
  ariaLabel,
  className,
}: Props<T>) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);

  // Reset scroll when the list shrinks past the current scroll offset (e.g.
  // a filter pruned items below the current viewport).
  useEffect(() => {
    const max = Math.max(0, items.length * rowHeight - height);
    if (scrollTop > max && ref.current) {
      ref.current.scrollTop = max;
      setScrollTop(max);
    }
  }, [items.length, rowHeight, height, scrollTop]);

  const totalHeight = items.length * rowHeight;
  const startIdx = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const visibleCount = Math.ceil(height / rowHeight) + overscan * 2;
  const endIdx = Math.min(items.length, startIdx + visibleCount);
  const offsetY = startIdx * rowHeight;

  const visible = items.slice(startIdx, endIdx);

  return (
    <div
      ref={ref}
      role="region"
      aria-label={ariaLabel}
      onScroll={(e) => setScrollTop((e.target as HTMLDivElement).scrollTop)}
      style={{ height, overflowY: "auto", position: "relative" }}
      className={className}
      data-testid="virtual-list"
    >
      <div style={{ height: totalHeight, position: "relative" }}>
        <div
          style={{
            position: "absolute",
            top: offsetY,
            left: 0,
            right: 0,
          }}
        >
          {visible.map((item, i) => (
            <div key={startIdx + i} style={{ height: rowHeight }}>
              {renderRow(item, startIdx + i)}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
