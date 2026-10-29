export interface Declined {
  readonly _tag: "Declined";
  readonly reason: string;
}
export type GatewayFailure<T extends string> = {
  readonly _tag: T;
  readonly retryAfter: number;
};
export type GatewayError = Declined | GatewayFailure<"RateLimited">;
