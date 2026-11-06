import { spawn } from "node:child_process";
import { createRequire } from "node:module";

export async function resolveNativeBinary(transport) {
  return transport.binary ?? (await import(createRequire(import.meta.url).resolve("@errspec/cli/native"))).nativeBinary();
}

export function jsonInput(value, message) {
  return JSON.stringify(value, (_key, value) => {
    if (["function", "symbol", "bigint"].includes(typeof value) || typeof value === "number" && !Number.isFinite(value)) {
      throw new TypeError(message);
    }
    return value;
  });
}

/** Exchange one JSON document. Each API owns response validation and exit-code meaning. */
export function exchangeNative({ binary, command, cwd, input, label, outputLimit, outputError, signal, interrupt = false }) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, [command], { stdio: ["pipe", "pipe", "inherit"], cwd });
    let output = "", failure, killTimer;
    const stop = () => {
      if (interrupt) {
        child.kill("SIGINT");
        killTimer ??= setTimeout(() => child.kill("SIGKILL"), 5000).unref();
      } else child.kill("SIGKILL");
    };
    signal?.addEventListener("abort", stop, { once: true });
    if (signal?.aborted) stop();
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", chunk => {
      if (failure) return;
      output += chunk;
      if (Buffer.byteLength(output) > outputLimit) {
        failure = new Error(outputError);
        output = "";
        stop();
      }
    });
    child.stdin.on("error", error => { if (error.code !== "EPIPE") failure = error; });
    child.on("error", error => { failure = error; });
    child.once("close", (code, signalName) => {
      signal?.removeEventListener("abort", stop);
      clearTimeout(killTimer);
      try {
        if (failure) throw failure;
        if (signalName) throw new Error(`${label} terminated by ${signalName}`);
        resolve({ code, response: JSON.parse(output) });
      } catch (error) { reject(error); }
    });
    child.stdin.end(input);
  });
}
