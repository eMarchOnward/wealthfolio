import { describe, expect, it } from "vitest";
import { histogramBins } from "./tax-plan-risk";

describe("histogramBins", () => {
  it("spreads the values over about √n equal bins, the largest in the last", () => {
    const values = [10, 20, 30, 40, 50, 60, 70, 80, 90];
    const bins = histogramBins(values);
    expect(bins).toHaveLength(3);
    expect(bins[0]).toMatchObject({ from: 10, count: 3 });
    expect(bins[2]).toMatchObject({ to: 90, count: 3 });
    expect(bins.reduce((sum, bin) => sum + bin.count, 0)).toBe(values.length);
  });

  it("caps the bins at 20 and keeps equal values in one", () => {
    expect(histogramBins(Array.from({ length: 1000 }, (_, k) => k))).toHaveLength(20);
    expect(histogramBins([5, 5, 5])).toEqual([{ from: 5, to: 5, count: 3 }]);
    expect(histogramBins([])).toEqual([]);
  });
});
