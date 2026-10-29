import type { PaymentError } from "./errors";

type ChargeResult = { ok: true } | { ok: false; error: PaymentError };
export interface Dependencies {
  charge(): Promise<ChargeResult>;
  createOrder(): Promise<void>;
}

export async function checkout(deps: Dependencies, maxAttempts = 1): Promise<{ status: number }> {
  for (let attempt = 1; ; attempt++) {
    const payment = await deps.charge();
    if (payment.ok) {
      await deps.createOrder();
      return { status: 201 };
    }
    if (payment.error._tag === "Declined") return { status: 402 };
    if (attempt >= maxAttempts) return { status: 429 };
  }
}
