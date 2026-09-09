import type { FissureVariant } from "./settings";

export interface WsCycle { expiry: string; }
export interface WsCetus extends WsCycle { isDay: boolean; }
export interface WsVallis extends WsCycle { isWarm: boolean; }
export interface WsCambion extends WsCycle { active: string; }
export interface WsZariman extends WsCycle { active: boolean; }

export interface WsSortieVariant { missionType: string; modifier: string; node: string; }
export interface WsSortie { expiry: string; boss: string; faction: string; variants: WsSortieVariant[]; active: boolean; }
export interface WsMission { type: string; node: string; }
export interface WsArchon { expiry: string; boss: string; faction: string; missions: WsMission[]; active: boolean; }
export interface WsManifestItem { name: string; uniqueName?: string; primePrice?: number; regularPrice?: number; ayaPrice?: number; regalAyaPrice?: number; }
export interface WsTrader { expiry: string; activation: string; character: string; location: string; active: boolean; manifest: WsManifestItem[]; }
export interface WsPrimeResurgence { expiry: string; activation: string; active: boolean; manifest: WsManifestItem[]; }
export interface WsNight { expiry: string; season: number; active: boolean; }
export interface WsFissure { id: string; expiry: string; node: string; missionType: string; enemy: string; tier: string; tierNum: number; isStorm: boolean; isHard: boolean; active: boolean; }
export interface WsStorm { id: string; expiry: string; node: string; missionType: string; enemy: string; tier: string; tierNum: number; active: boolean; }
export type MatchedFissure = { f: WsFissure | WsStorm; variant: FissureVariant };
export type SeenFissures = Map<string, Set<string>>;
export interface WsAlert { id: string; expiry: string; missionType: string; faction: string; node: string; rewardItem?: string; rewardCredits: number; }
export interface WsInvasion { id: string; node: string; attacker: string; defender: string; attReward: string; defReward: string; pct: number; }
export interface WsDarvo { expiry: string; item: string; discount: number; originalPrice: number; salePrice: number; amountTotal: number; amountSold: number; }
export interface WsCircuit { expiry: string; normalFrames: string[]; hardWeapons: string[]; }
export interface WsSimple { expiry: string; }
export interface WsBounty { expiry: string; jobCount: number; }
export interface WsEvent { expiry: string; label: string; }
export interface WsNews { message: string; link: string; date: string | number; stream: boolean; primeAccess: boolean; update: boolean; }

export interface WorldState {
  cetus?: WsCetus;
  vallis?: WsVallis;
  cambion?: WsCambion;
  zariman?: WsZariman;
  bounties?: Record<string, WsBounty>;
  sortie?: WsSortie;
  archonHunt?: WsArchon;
  voidTrader?: WsTrader;
  nightwave?: WsNight;
  primeResurgence?: WsPrimeResurgence;
  circuit?: WsCircuit;
  kahl?: WsSimple;
  deepArchimedea?: WsSimple;
  events?: WsEvent[];
  news?: WsNews[];
  darvo?: WsDarvo;
  alerts?: WsAlert[];
  invasions?: WsInvasion[];
  fissures?: WsFissure[];
  spFissures?: WsFissure[];
  voidStorms?: WsStorm[];
}
