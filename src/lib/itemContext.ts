export function extractItemName(e: React.MouseEvent): string | null {
  const card = (e.target as HTMLElement).closest(".inv-card");
  if (!card) return null;
  const nameEl = card.querySelector(".inv-card-name, .inv-row-name");
  return nameEl?.textContent?.trim() || (card as HTMLElement).title?.split(" (")[0]?.trim() || null;
}
