export type Delivery = { accepted: boolean };

// The application imports this directly; tests replace the module instead of injecting it.
export async function deliver(to: string): Promise<Delivery> {
  const response = await fetch("https://mail.example.invalid/send", { method: "POST", body: to });
  return { accepted: response.ok };
}
