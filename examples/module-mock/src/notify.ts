import { deliver } from "./mailer";

export type NotifyResult =
  | { status: "sent" }
  | { status: "rejected" }
  | { status: "retry_later" };

export async function notify(to: string): Promise<NotifyResult> {
  try {
    const delivery = await deliver(to);
    return delivery.accepted ? { status: "sent" } : { status: "rejected" };
  } catch {
    return { status: "retry_later" };
  }
}
