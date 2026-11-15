// How a count is written in the console's footer and decks.
export const kilo = (n: number) => (n >= 10_000 ? `${(n / 1000).toFixed(1)}k` : String(n));
