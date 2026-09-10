export function splitSearchTerms(value: string): string[] {
  return value.split(",").map(term => term.trim().toLowerCase()).filter(Boolean);
}

export function matchesSearchTerms(terms: readonly string[], ...values: readonly string[]): boolean {
  return terms.length === 0 || terms.some(term => values.some(value => value.toLowerCase().includes(term)));
}
