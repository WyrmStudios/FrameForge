export default function Sparkline({ values, empty }: { values: number[]; empty: string }) {
  const W = 300, H = 60;
  const pt = 6, pb = 6, pl = 2, pr = 2;
  const cW = W - pl - pr;
  const cH = H - pt - pb;

  if (values.length === 0) {
    return <div className="flex h-15 items-center justify-center rounded-4 bg-black/10 text-11 italic text-muted">{empty}</div>;
  }

  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;

  const xi = (i: number) =>
    pl + (values.length === 1 ? cW / 2 : (i / (values.length - 1)) * cW);
  const yi = (v: number) => pt + (1 - (v - min) / range) * cH;

  if (values.length === 1) {
    return (
      <svg viewBox={`0 0 ${W} ${H}`} className="block h-15 w-full rounded-4 bg-black/15" preserveAspectRatio="none">
        <line
          x1={pl} y1={H / 2} x2={pl + cW} y2={H / 2}
          stroke="var(--accent)" strokeWidth="1" strokeOpacity="0.4" strokeDasharray="4 3"
        />
        <circle cx={xi(0)} cy={H / 2} r="3" fill="var(--accent)" />
      </svg>
    );
  }

  const linePts = values.map((v, i) => `${xi(i).toFixed(1)},${yi(v).toFixed(1)}`).join(" ");
  const fillPts = `${pl},${pt + cH} ${linePts} ${pl + cW},${pt + cH}`;

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="block h-15 w-full rounded-4 bg-black/15" preserveAspectRatio="none">
      <polygon points={fillPts} fill="var(--accent)" fillOpacity="0.12" />
      <polyline
        points={linePts}
        fill="none"
        stroke="var(--accent)"
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}
