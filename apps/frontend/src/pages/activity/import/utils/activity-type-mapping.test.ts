import { describe, expect, it } from "vitest";
import { ActivityType } from "@/lib/constants";
import { createDefaultActivityTemplate } from "./default-activity-template";
import { findMappedActivityType } from "./activity-type-mapping";

describe("activity-type-mapping", () => {
  it.each(Object.values(ActivityType).filter((type) => type !== ActivityType.UNKNOWN))(
    "treats exact canonical %s labels as explicit identity mappings",
    (type) => {
      expect(findMappedActivityType(type, createDefaultActivityTemplate().activityMappings)).toBe(
        type,
      );
    },
  );

  it("does not infer non-canonical labels without explicit mappings", () => {
    expect(findMappedActivityType("TRANSFER OUT", {})).toBeNull();
    expect(findMappedActivityType("PURCHASE", {})).toBeNull();
    expect(findMappedActivityType("DIV", {})).toBeNull();
  });

  it("requires exact explicit label matches", () => {
    expect(
      findMappedActivityType("DIVIDEND QUALIFIED", {
        [ActivityType.DIVIDEND]: ["DIVIDEND"],
      }),
    ).toBeNull();
  });

  it("lets explicit mappings override matching labels", () => {
    const mapped = findMappedActivityType("TRANSFER OUT", {
      [ActivityType.DEPOSIT]: ["TRANSFER OUT"],
    });
    expect(mapped).toBe(ActivityType.DEPOSIT);
  });

  it("preserves legacy truncated mappings when the old prefix cuts mid-label", () => {
    expect(
      findMappedActivityType("DIVIDEND QUALIFIED", {
        [ActivityType.DIVIDEND]: ["DIVIDEND_QUA"],
      }),
    ).toBe(ActivityType.DIVIDEND);
  });

  it("does not treat exact 12-character labels as legacy prefixes", () => {
    expect(
      findMappedActivityType("TRANSFER OUT FEE", {
        [ActivityType.TRANSFER_OUT]: ["TRANSFER_OUT"],
      }),
    ).toBeNull();
  });

  it("matches labels with variable suffixes through trailing-wildcard mappings", () => {
    const mappings = {
      [ActivityType.TRANSFER_IN]: ["TRANSFER FROM *"],
      [ActivityType.TRANSFER_OUT]: ["TRANSFER TO *"],
    };
    expect(findMappedActivityType("TRANSFER FROM 12345678", mappings)).toBe(
      ActivityType.TRANSFER_IN,
    );
    expect(findMappedActivityType("Transfer to 87654321", mappings)).toBe(
      ActivityType.TRANSFER_OUT,
    );
    expect(findMappedActivityType("TRANSFER", mappings)).toBeNull();
  });

  it("prefers exact matches over wildcard matches", () => {
    expect(
      findMappedActivityType("TRANSFER FROM CASH", {
        [ActivityType.TRANSFER_IN]: ["TRANSFER FROM *"],
        [ActivityType.DEPOSIT]: ["TRANSFER FROM CASH"],
      }),
    ).toBe(ActivityType.DEPOSIT);
  });

  it("treats the selected template as the source of truth", () => {
    expect(
      findMappedActivityType("BUY", {
        [ActivityType.DIVIDEND]: ["DIV"],
      }),
    ).toBeNull();
  });
});
