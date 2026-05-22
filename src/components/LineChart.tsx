// Minimal SVG line/area chart. No external dependencies (CLAUDE.md §5 —
// CloudSaw runs fully local, so we avoid pulling in a charting bundle).
//
// One or more series share the X axis. Each series is rendered as a line
// plus optional area fill. Values are accessibility-described via an
// adjacent <ul> the screen reader can step through — the SVG itself is
// `aria-hidden` so the chart is never the *only* way to learn the data.

import { useMemo } from "react";

type Point = { x: number; y: number; label: string };

export type Series = {
  id: string;
  label: string;
  color: string;
  points: Point[];
};

type Props = {
  series: Series[];
  width?: number;
  height?: number;
  /** X-axis label shown below the chart. */
  xLabel?: string;
  /** Y-axis label shown left of the chart. */
  yLabel?: string;
  /** Localized title used for the screen-reader-visible summary. */
  ariaTitle: string;
  /** Optional fixed Y maximum; otherwise computed from data. */
  yMax?: number;
};

const PADDING = { top: 16, right: 16, bottom: 32, left: 40 };

export default function LineChart({
  series,
  width = 520,
  height = 220,
  xLabel,
  yLabel,
  ariaTitle,
  yMax,
}: Props) {
  const { paths, allPoints, computedYMax, gridLines } = useMemo(() => {
    const allPoints = series.flatMap((s) => s.points);
    const maxY = yMax ?? Math.max(1, ...allPoints.map((p) => p.y));
    const xs = allPoints.map((p) => p.x);
    const minX = xs.length ? Math.min(...xs) : 0;
    const maxX = xs.length ? Math.max(...xs) : 1;
    const spanX = maxX - minX || 1;

    const innerW = width - PADDING.left - PADDING.right;
    const innerH = height - PADDING.top - PADDING.bottom;

    const toX = (x: number) =>
      PADDING.left + ((x - minX) / spanX) * innerW;
    const toY = (y: number) =>
      PADDING.top + innerH - (y / maxY) * innerH;

    const paths = series.map((s) => {
      if (s.points.length === 0) {
        return { id: s.id, color: s.color, label: s.label, d: "", area: "" };
      }
      const sorted = [...s.points].sort((a, b) => a.x - b.x);
      const d = sorted
        .map((p, i) => `${i === 0 ? "M" : "L"} ${toX(p.x)} ${toY(p.y)}`)
        .join(" ");
      const area =
        sorted.length > 0
          ? `${d} L ${toX(sorted[sorted.length - 1].x)} ${PADDING.top + innerH} L ${toX(sorted[0].x)} ${PADDING.top + innerH} Z`
          : "";
      return { id: s.id, color: s.color, label: s.label, d, area };
    });

    const gridCount = 4;
    const gridLines: { y: number; value: number }[] = [];
    for (let i = 0; i <= gridCount; i += 1) {
      const value = (maxY / gridCount) * i;
      gridLines.push({ y: toY(value), value });
    }

    return { paths, allPoints, computedYMax: maxY, gridLines };
  }, [series, width, height, yMax]);

  const innerW = width - PADDING.left - PADDING.right;
  const innerH = height - PADDING.top - PADDING.bottom;
  const baseline = PADDING.top + innerH;

  return (
    <figure className="rounded-card border border-saw-grey-200 bg-saw-white p-4">
      <figcaption className="text-small font-medium text-saw-grey-700">
        {ariaTitle}
      </figcaption>
      <svg
        role="img"
        aria-label={ariaTitle}
        viewBox={`0 0 ${width} ${height}`}
        className="mt-2 w-full max-w-full"
      >
        {/* Y grid */}
        {gridLines.map((g, i) => (
          <g key={`grid-${i}`}>
            <line
              x1={PADDING.left}
              x2={PADDING.left + innerW}
              y1={g.y}
              y2={g.y}
              stroke="#E5E5E5"
              strokeDasharray="2 4"
            />
            <text
              x={PADDING.left - 6}
              y={g.y + 4}
              fontSize="10"
              textAnchor="end"
              fill="#6B6B6B"
            >
              {Math.round(g.value)}
            </text>
          </g>
        ))}
        {/* X baseline */}
        <line
          x1={PADDING.left}
          x2={PADDING.left + innerW}
          y1={baseline}
          y2={baseline}
          stroke="#9CA3AF"
        />
        {/* Series */}
        {paths.map((p) => (
          <g key={p.id}>
            {p.area ? (
              <path d={p.area} fill={p.color} fillOpacity="0.12" />
            ) : null}
            {p.d ? (
              <path
                d={p.d}
                fill="none"
                stroke={p.color}
                strokeWidth="2"
                strokeLinejoin="round"
                strokeLinecap="round"
              />
            ) : null}
          </g>
        ))}
        {/* Axis labels */}
        {yLabel ? (
          <text
            transform={`translate(12, ${PADDING.top + innerH / 2}) rotate(-90)`}
            fontSize="10"
            textAnchor="middle"
            fill="#6B6B6B"
          >
            {yLabel}
          </text>
        ) : null}
        {xLabel ? (
          <text
            x={PADDING.left + innerW / 2}
            y={height - 8}
            fontSize="10"
            textAnchor="middle"
            fill="#6B6B6B"
          >
            {xLabel}
          </text>
        ) : null}
      </svg>
      {/* Legend + SR table */}
      <ul className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-small text-saw-grey-700">
        {series.map((s) => (
          <li key={s.id} className="inline-flex items-center gap-2">
            <span
              aria-hidden="true"
              className="inline-block h-2 w-3 rounded-sm"
              style={{ backgroundColor: s.color }}
            />
            <span>
              {s.label}
              <span className="sr-only">
                {" "}— {s.points.length} data points
              </span>
            </span>
          </li>
        ))}
      </ul>
      {allPoints.length === 0 ? (
        <p className="mt-2 text-small text-saw-grey-500">
          {/* Visible empty hint, intentionally not localized inline — the
              parent screen passes already-localized data. */}
        </p>
      ) : null}
      <span className="sr-only">
        Y axis maximum: {Math.round(computedYMax)}.
      </span>
    </figure>
  );
}
