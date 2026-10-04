import UpdateBadge from "./UpdateBadge";

const MASTERY_BADGE =
  "text-11 font-bold text-mastery bg-mastery/12 border border-mastery/30 rounded-5 px-2 py-0.5 shrink-0 tracking-0.03";
const PLAYER_NAME_BADGE =
  "text-11 font-semibold text-player-name bg-white/6 border border-white/14 rounded-5 px-2 py-0.5 shrink-0 tracking-0.02";
const BLOB_STATUS_BADGE =
  "text-11 font-semibold rounded-5 px-2.25 py-0.5 shrink-0 tracking-0.02 text-success bg-success/10 border border-success/30";

interface HeaderStatusBadgesProps {
  masteryRank: number | null;
  playerName: string | null;
  pendingUpdate: string | null;
  updateInstalling: boolean;
  inventoryLoaded: boolean;
  onInstallUpdate: () => void;
  onDismissUpdate: () => void;
}

export default function HeaderStatusBadges({
  masteryRank, playerName, pendingUpdate, updateInstalling, inventoryLoaded, onInstallUpdate, onDismissUpdate,
}: HeaderStatusBadgesProps) {
  return (
    <>
      {masteryRank !== null && <span className={MASTERY_BADGE} title="Mastery Rank">MR {masteryRank}</span>}
      {playerName && <span className={PLAYER_NAME_BADGE} title="Logged-in Warframe account">{playerName}</span>}
      {pendingUpdate && <UpdateBadge version={pendingUpdate} installing={updateInstalling} onInstall={onInstallUpdate} onDismiss={onDismissUpdate} />}
      {inventoryLoaded && <span className={BLOB_STATUS_BADGE} title="Inventory loaded from Warframe memory">Inventory Loaded</span>}
    </>
  );
}
