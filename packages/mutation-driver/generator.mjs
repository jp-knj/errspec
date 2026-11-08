// Private sequential transport; callers own one instance per driver.
import { spawn } from "node:child_process";
const LIMIT = 64 * 1024 * 1024;
const failure = (message) =>
  Object.assign(new Error(message), { code: "EC_MUTATION_PROTOCOL" });
export class Generator {
  constructor(binary, { timeout = 60000, maxBuffer = LIMIT } = {}) {
    this.timeout = timeout;
    this.maxBuffer = maxBuffer;
    this.id = 0;
    this.pending = null;
    this.buffer = Buffer.alloc(0);
    this.stderrBytes = 0;
    this.failure = null;
    this.closing = false;
    this.child = spawn(binary, ["__mutation-source-lines"], {
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.exited = new Promise((resolve) =>
      this.child.once("close", (code, signal) => {
        if (!this.closing || this.pending || code !== 0)
          this.fail(failure(`Generator exited (${code ?? signal})`));
        resolve();
      }),
    );
    this.child.on("error", (error) =>
      this.fail(failure(`Generator start failed: ${error.message}`)),
    );
    this.child.stdin.on("error", (error) =>
      this.fail(failure(`Generator input failed: ${error.message}`)),
    );
    this.child.stdout.on("error", (error) =>
      this.fail(failure(`Generator output failed: ${error.message}`)),
    );
    this.child.stderr.on("error", (error) =>
      this.fail(failure(`Generator stderr failed: ${error.message}`)),
    );
    this.child.stderr.on("data", (chunk) => {
      this.stderrBytes += chunk.length;
      if (this.stderrBytes > this.maxBuffer)
        this.fail(failure("Generator stderr exceeds 64 MiB"));
    });
    this.child.stdout.on("data", (chunk) => this.receive(chunk));
  }
  fail(error) {
    this.failure ??= error;
    if (this.pending) {
      clearTimeout(this.pending.timer);
      this.pending.reject(this.failure);
      this.pending = null;
    }
    this.child.kill("SIGKILL");
  }
  receive(chunk) {
    if (this.failure) return;
    if (!this.pending) {
      this.fail(failure("Unsolicited generator response"));
      return;
    }
    if (this.buffer.length + chunk.length > this.maxBuffer) {
      this.fail(failure("Generator response exceeds 64 MiB"));
      return;
    }
    this.buffer = Buffer.concat([this.buffer, chunk]);
    const end = this.buffer.indexOf(10);
    if (end < 0) return;
    if (end !== this.buffer.length - 1) {
      this.fail(failure("Multiple generator responses"));
      return;
    }
    let frame;
    try {
      frame = JSON.parse(
        new TextDecoder("utf-8", { fatal: true }).decode(this.buffer),
      );
    } catch {
      this.fail(failure("Invalid generator JSON"));
      return;
    }
    this.buffer = Buffer.alloc(0);
    if (
      !frame ||
      typeof frame !== "object" ||
      Array.isArray(frame) ||
      frame.version !== 1 ||
      frame.id !== this.pending.id ||
      !!frame.result === !!frame.error ||
      (frame.result && !Array.isArray(frame.result.candidates)) ||
      (frame.error &&
        (!["syntax", "input"].includes(frame.error.kind) ||
          typeof frame.error.message !== "string"))
    ) {
      this.fail(failure("Invalid generator response"));
      return;
    }
    const pending = this.pending;
    this.pending = null;
    clearTimeout(pending.timer);
    if (frame.error)
      pending.reject(
        Object.assign(new Error(frame.error.message), {
          code: frame.error.kind === "input" ? "EC_MUTATION_INPUT" : "EC_MUTATION_INSTRUMENTATION",
        }),
      );
    else pending.resolve(frame.result);
  }
  async call(request) {
    if (this.failure) throw this.failure;
    if (this.closing || this.pending)
      throw failure("Generator requires sequential requests");
    const id = ++this.id,
      frame = JSON.stringify({ version: 1, id, request }) + "\n";
    if (Buffer.byteLength(frame) > this.maxBuffer)
      throw failure("Generator request exceeds 64 MiB");
    this.stderrBytes = 0;
    try {
      return await new Promise((resolve, reject) => {
        const timer = setTimeout(
          () => this.fail(failure("Generator deadline exceeded")),
          this.timeout,
        );
        this.pending = { id, resolve, reject, timer };
        this.child.stdin.write(frame);
      });
    } catch (error) {
      if (this.failure) await this.close();
      throw error;
    }
  }
  async close() {
    if (this.closePromise) return this.closePromise;
    this.closing = true;
    if (this.pending) this.fail(failure("Generator interrupted"));
    this.child.stdin.end();
    const timer = setTimeout(() => this.child.kill("SIGKILL"), 1000);
    this.closePromise = this.exited.finally(() => clearTimeout(timer));
    return this.closePromise;
  }
}
