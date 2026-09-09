export type TradeType = "sale" | "purchase" | "trade";

export interface TradeItem {
  name: string;
  qty: number;
}

export interface Trade {
  id: number;
  timestamp: string;
  with_player: string;
  direction: "sold" | "bought" | "traded-out" | "traded-in";
  item_name: string;
  item_url: string;
  quantity: number;
  platinum: number;
  source: string;
  notes: string;
  session_id: string;
  trade_type: string;
}

export interface TradeCompletedEvent {
  sessionId: string;
  withPlayer: string;
  tradeType: TradeType;
  offeredItems: TradeItem[];
  offeredPlat: number;
  receivedItems: TradeItem[];
  receivedPlat: number;
  timestamp: string;
}

export interface TradeSession {
  sessionId: string;
  withPlayer: string;
  tradeType: TradeType;
  givenItems: TradeItem[];
  givenPlat: number;
  receivedItems: TradeItem[];
  receivedPlat: number;
  timestamp: string;
}
