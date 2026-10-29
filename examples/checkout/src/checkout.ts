import type { PaymentError } from "./errors";

type ChargeResult = { ok: true } | { ok: false; error: PaymentError };
export interface Dependencies {
  charge(): Promise<ChargeResult>;
  createOrder(): Promise<void>;
}

export async function checkout(deps: Dependencies): Promise<{ status: number }> {
  const payment = await deps.charge();
  if (!payment.ok) {
    switch (payment.error._tag) {
      case "Declined": return { status: 402 };
      case "RateLimited": return { status: 429 };
    }
  }
  await deps.createOrder();
  return { status: 201 };
}
