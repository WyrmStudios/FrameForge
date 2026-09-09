interface ConnectionStatusChipProps {
  label: string;
  state: "online" | "warn" | "offline" | "disabled";
  detail: string;
  title: string;
  onClick?: () => void | Promise<void>;
}

export default function ConnectionStatusChip({ label, state, detail, title, onClick }: ConnectionStatusChipProps) {
  const content = <>
      <span className="conn-dot" />
      <span className="conn-label">{label}</span>
      <span className="conn-detail">{detail}</span>
    </>;

  if (onClick) {
    return <button type="button" className={`conn-chip conn-chip-button conn-${state}`} title={title} onClick={() => { onClick(); }}>{content}</button>;
  }
  return <span className={`conn-chip conn-${state}`} title={title}>{content}</span>;
}
