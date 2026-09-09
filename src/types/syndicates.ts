export interface SyndicateItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
  tier: string;
  ducats?: number;
  owned: number;
  result_unique?: string;
  result_owned: number;
}

export interface SyndicateStore {
  name: string;
  items: SyndicateItem[];
}
