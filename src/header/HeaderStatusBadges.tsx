import UpdateBadge from "./UpdateBadge";

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
      {masteryRank !== null && <span className="mastery-badge" title="Mastery Rank">MR {masteryRank}</span>}
      {playerName && <span className="player-name-badge" title="Logged-in Warframe account">{playerName}</span>}
      {pendingUpdate && <UpdateBadge version={pendingUpdate} installing={updateInstalling} onInstall={onInstallUpdate} onDismiss={onDismissUpdate} />}
      {inventoryLoaded && <span className="blob-status-badge blob-status-done" title="Inventory loaded from Warframe memory">Inventory Loaded</span>}
    </>
  );
}
