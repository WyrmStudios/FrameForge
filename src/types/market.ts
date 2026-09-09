export interface WfmItem {
  id: string;
  item_name: string;
  url_name: string;
}

/** Item detail returned by wfm_get_item_info. */
export interface WfmItemInfo {
  id: string;
  modMaxRank?: number;
}

export interface WfmPrice {
  url_name: string;
  sell_median?: number | null;
  buy_median?: number | null;
  tradeable?: boolean;
}

export interface WfmPriceUpdate {
  url_name: string;
  sell_median: number | null;
  tradeable: boolean;
}

export type WfmCachedPrices = Record<string, number | null>;

export interface WfmItemOrders {
  sell: WfmPublicOrder[];
  buy: WfmPublicOrder[];
}

export interface BlobRivenStat {
  tag: string;
  value: number;
}

export interface BlobRivenEntry {
  item_id: string;
  item_type: string;
  mod_name: string;
  riven_state: "unrevealed" | "revealed" | "unlocked";
  compat: string | null;
  challenge_type: string | null;
  challenge_complication: string | null;
  lvl_req: number | null;
  polarity: string | null;
  buffs: BlobRivenStat[];
  curses: BlobRivenStat[];
  mod_rank: number;
  count: number;
  rerolls: number;
}

export interface WfmPublicOrder {
  id?: string;
  platinum: number;
  quantity: number;
  user: { ingameName: string; reputation: number; status: string };
  mod_rank?: number;
}

export interface WfmManagedOrder {
  id: string;
  itemId?: string;
  type: "sell" | "buy";
  platinum: number;
  quantity: number;
  visible: boolean;
  item?: { slug?: string; urlName?: string; url_name?: string; en?: { item_name: string }; i18n?: { en?: { name: string } } };
}

export interface WfmWhisper {
  from: string;
  message: string;
  item?: string;
  price?: number;
  timestamp: string;
  completedAt?: number;
  revertInfo?: {
    orderId: string;
    itemId: string;
    platinum: number;
    originalQty: number;
    newQty: number;
    visible: boolean;
  };
}

export interface WfmRivenAttribute {
  url_name: string;
  positive: boolean;
  value: number;
}

export interface WfmAuction {
  id: string;
  starting_price: number;
  buyout_price: number | null;
  top_bid: number | null;
  bids: number;
  winner: { ingame_name: string } | null;
  is_closed: boolean;
  is_direct_sell: boolean;
  visible: boolean;
  note: string;
  minimal_reputation: number;
  item: {
    weapon_url_name: string;
    name: string;
    mastery_level: number;
    mod_rank: number;
    re_rolls: number;
    polarity: string;
    attributes: WfmRivenAttribute[];
  };
}

export interface WfmStatPoint {
  datetime: string;
  median: number;
  volume: number;
}

export interface WfmTopItem {
  name: string;
  url_name: string;
  image_name?: string;
  unit_price: number;
  daily_volume: number;
  total_value_7d: number;
}
