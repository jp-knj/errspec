import { describe, expect, it, vi } from "vitest";
import { checkout } from "../src/checkout";

describe("checkout", () => {
  it("Declined returns 402 without creating an order", async () => {
    const createOrder = vi.fn(async () => {});
    const result = await checkout({
      charge: async () => ({ ok: false, error: { _tag: "Declined", reason: "insufficient funds" } }),
      createOrder,
    });
    expect(result.status).toBe(402);
    expect(createOrder).not.toHaveBeenCalled();
  });

  it("RateLimited returns 429 without creating an order", async () => {
    const createOrder = vi.fn(async () => {});
    const result = await checkout({
      charge: async () => ({ ok: false, error: { _tag: "RateLimited", retryAfter: 30 } }),
      createOrder,
    });
    expect(result.status).toBe(429);
    expect(createOrder).not.toHaveBeenCalled();
  });

  it("success creates exactly one order", async () => {
    const createOrder = vi.fn(async () => {});
    const result = await checkout({ charge: async () => ({ ok: true }), createOrder });
    expect(result.status).toBe(201);
    expect(createOrder).toHaveBeenCalledTimes(1);
  });
});
