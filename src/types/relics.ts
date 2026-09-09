export interface DropReward {
  itemName: string;
  chance: number;
  rarity: string;
}

export interface RelicDrop {
  tier: string;
  relicName: string;
  fullName: string;
  rewards: DropReward[];
}

export interface RelicPickReward {
  name: string;
  rarity: string;
  drop_rate: number;
  ducats: number;
  plat: number;
  vaulted: boolean;
  owned: boolean;
}

export interface RelicPickRelic {
  name: string;
  base_name: string;
  refinement: string;
  count: number;
  unowned_score: number;
  ducat_score: number;
  plat_score: number;
  rewards: RelicPickReward[];
}

export interface RelicPickPayload {
  era: string;
  relics: RelicPickRelic[];
}
