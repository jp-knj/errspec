export type SubmitResult =
  | { status: "invalid" }
  | { status: "blocked" }
  | { status: "sent" };

export interface Dependencies {
  verifyHuman(): Promise<boolean>;
  send(email: string): Promise<void>;
}

export async function submit(email: string, deps: Dependencies): Promise<SubmitResult> {
  if (!email.includes("@")) {
    return { status: "invalid" };
  }
  let human: boolean;
  try {
    human = await deps.verifyHuman();
  } catch {
    return { status: "blocked" };
  }
  if (!human) {
    return { status: "blocked" };
  }
  await deps.send(email);
  return { status: "sent" };
}
