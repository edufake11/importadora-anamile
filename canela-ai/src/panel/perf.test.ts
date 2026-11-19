import { describe, expect, it } from "vitest";
import { fmtGb } from "../lib/ipc";
import { expiresIn } from "./PerformanceSettings";

describe("rendimiento y memoria", () => {
  it("formatea GB", () => {
    expect(fmtGb(7.5 * 1024 ** 3)).toBe("7,5 GB");
  });

  it("dice cuándo se descarga el modelo", () => {
    const now = Date.parse("2026-11-19T10:00:00Z");
    expect(expiresIn("2026-11-19T10:01:32Z", now)).toBe("se descarga en 1:32");
    expect(expiresIn("2318-01-01T00:00:00Z", now)).toBe("no se descarga solo");
    expect(expiresIn(null, now)).toBeNull();
    expect(expiresIn("2026-11-19T09:00:00Z", now)).toBeNull();
  });
});
