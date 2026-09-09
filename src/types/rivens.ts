export interface RivenAlternativeResult {
  label: string;
  matched: string[];
  missing: string[];
  score: number;
  verdict: string;
}

export interface RivenAnalysis {
  weapon: string;
  matched_positives: string[];
  missing_positives: string[];
  safe_negatives_present: string[];
  harmful_negatives: string[];
  total_wanted: number;
  score: number;
  verdict: string;
  notes: string;
  alternatives: RivenAlternativeResult[];
}

export interface SavedRiven {
  id: string;
  weapon: string;
  label: string;
  stats_json: string;
  verdict: string;
  score: number;
  saved_at: string;
}

export interface RivenStat {
  name: string;
  value: string;
  positive: boolean;
  useMultiplier?: boolean;
}

export interface RivenAnalysisUpdate {
  analysis: RivenAnalysis | null;
  rollCount: number;
  ocrRaw?: string;
  weapon?: string;
  positives?: string[];
  negatives?: string[];
  rolledStats?: RivenStat[];
  originalStats?: RivenStat[];
  isComparison?: boolean;
}
