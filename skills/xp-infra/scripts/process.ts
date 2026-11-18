import { spawn } from "node:child_process";
import { ensure } from "./input.ts";

export interface CommandOptions {
  input?: string;
  timeoutMs?: number;
  limit?: number;
  env?: NodeJS.ProcessEnv;
}
export async function boundedInput(
  stream: ReadableStream<Uint8Array>,
  timeoutMs = 15000,
): Promise<string> {
  const reader = stream.getReader(),
    chunks: Uint8Array[] = [];
  let count = 0;
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    void reader.cancel("input timed out");
  }, timeoutMs);
  let complete = false;
  try {
    while (true) {
      const next = await reader.read();
      if (next.done) {
        complete = true;
        break;
      }
      count += next.value.length;
      ensure(count <= 65536, "input exceeds limit");
      chunks.push(next.value);
    }
    ensure(complete && !timedOut, "input timed out or incomplete");
    return new TextDecoder("utf-8", { fatal: true }).decode(Buffer.concat(chunks));
  } finally {
    clearTimeout(timer);
    void reader.cancel();
    reader.releaseLock();
  }
}
export async function command(argv: string[], options: CommandOptions = {}): Promise<string> {
  const limit = options.limit ?? 65536;
  return await new Promise<string>((resolve, reject) => {
    const child = spawn(argv[0], argv.slice(1), {
      detached: true,
      stdio: ["pipe", "pipe", "pipe"],
      env: options.env ?? process.env,
    });
    const chunks: Buffer[] = [];
    let outputBytes = 0,
      errorBytes = 0,
      failure: Error | undefined;
    const terminate = () => {
      if (child.pid) {
        try {
          process.kill(-child.pid, "SIGKILL");
        } catch {
          child.kill("SIGKILL");
        }
      }
    };
    const fail = (message: string) => {
      failure ??= new Error(message);
      terminate();
    };
    const onSignal = () => fail("command interrupted");
    for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"] as const) process.on(signal, onSignal);
    const timer = setTimeout(() => fail("command timed out"), options.timeoutMs ?? 60000);
    child.stdout.on("data", (chunk: Buffer) => {
      outputBytes += chunk.length;
      if (outputBytes > limit) fail("command output exceeds limit");
      else chunks.push(chunk);
    });
    // Never echo remote stderr: it can contain secrets and arbitrary terminal data.
    child.stderr.on("data", (chunk: Buffer) => {
      errorBytes += chunk.length;
      if (errorBytes > limit) fail("command diagnostics exceed limit");
    });
    child.stdin.on("error", () => {
      /* Early remote failure is handled by exit status. */
    });
    child.on("error", () => {
      failure ??= new Error("system command unavailable");
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"] as const) process.off(signal, onSignal);
      if (failure) {
        reject(failure);
        return;
      }
      try {
        ensure(code === 0, "system command failed");
        resolve(new TextDecoder("utf-8", { fatal: true }).decode(Buffer.concat(chunks)));
      } catch (error) {
        reject(error);
      }
    });
    child.stdin.end(options.input ?? "");
  });
}
