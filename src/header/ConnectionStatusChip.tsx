import { CONN_CHIP, CONN_BUTTON, CONN_DOT, CONN_LABEL, CONN_STATUS } from "./connStatus";

interface ConnectionStatusChipProps {
  label: string;
  state: "online" | "warn" | "offline" | "disabled";
  detail: string;
  title: string;
  onClick?: () => void | Promise<void>;
}

export default function ConnectionStatusChip({ label, state, detail, title, onClick }: ConnectionStatusChipProps) {
  const s = CONN_STATUS[state];
  const content = <>
      <span className={`${CONN_DOT} ${s.dot}`} />
      <span className={CONN_LABEL}>{label}</span>
      <span className={s.detail}>{detail}</span>
    </>;

  if (onClick) {
    return <button type="button" className={`${CONN_CHIP} ${CONN_BUTTON} ${s.chip}`} title={title} onClick={() => { onClick(); }}>{content}</button>;
  }
  return <span className={`${CONN_CHIP} ${s.chip}`} title={title}>{content}</span>;
}
