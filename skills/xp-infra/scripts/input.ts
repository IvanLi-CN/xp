// Strict external-input parsing shared by policy, audit and preflight.
export function ensure(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
export type ObjectValue = Record<string, unknown>;
export function object(value: unknown, name: string): ObjectValue {
  ensure(
    value !== null && typeof value === "object" && !Array.isArray(value),
    `${name} must be an object`,
  );
  return value as ObjectValue;
}
export function fields(value: ObjectValue, allowed: readonly string[], name: string): void {
  ensure(
    Object.keys(value).every((key) => allowed.includes(key)),
    `unknown ${name} fields`,
  );
}
export function integer(value: string, min: number, max: number, name: string): number {
  ensure(/^(?:0|[1-9][0-9]*)$/.test(value), `invalid ${name}`);
  const n = Number(value);
  ensure(Number.isSafeInteger(n) && n >= min && n <= max, `invalid ${name}`);
  return n;
}
export function records(text: string, required: readonly string[] = []): Record<string, string> {
  const result: Record<string, string> = Object.create(null);
  ensure(
    Buffer.byteLength(text) <= 65536 && !/[\x00-\x08\x0b-\x1f\x7f]/.test(text),
    "invalid or oversized records",
  );
  for (const line of text.split("\n")) {
    if (!line) continue;
    const match = /^([a-z][a-z0-9_]*)=(.*)$/.exec(line);
    ensure(match && !Object.hasOwn(result, match[1]), "malformed or duplicate record");
    result[match[1]] = match[2];
  }
  for (const key of required) ensure(Object.hasOwn(result, key), `missing ${key}`);
  return result;
}

// Walk the JSON grammar before JSON.parse so escaped duplicate names are rejected
// at every depth. A reviver cannot observe members that JSON.parse overwrote.
export function strictJson(text: string): unknown {
  ensure(Buffer.byteLength(text) <= 65536, "JSON exceeds output limit");
  let offset = 0;
  const whitespace = () => {
    while (/[\x20\t\r\n]/.test(text[offset] ?? "!")) offset++;
  };
  function string(): string {
    const start = offset++;
    while (offset < text.length) {
      const char = text[offset++];
      if (char === "\\") offset++;
      else if (char === '"') {
        try {
          return JSON.parse(text.slice(start, offset)) as string;
        } catch {
          throw new Error("invalid JSON string");
        }
      }
    }
    throw new Error("unterminated JSON string");
  }
  function value(depth: number): void {
    ensure(depth <= 64, "JSON nesting exceeds limit");
    whitespace();
    const char = text[offset];
    if (char === "{" || char === "[") {
      const close = char === "{" ? "}" : "]";
      const names = new Set<string>();
      offset++;
      whitespace();
      if (text[offset] === close) {
        offset++;
        return;
      }
      while (true) {
        if (char === "{") {
          ensure(text[offset] === '"', "invalid JSON member");
          const name = string();
          ensure(!names.has(name), "duplicate JSON member");
          names.add(name);
          whitespace();
          ensure(text[offset++] === ":", "invalid JSON separator");
        }
        value(depth + 1);
        whitespace();
        if (text[offset] === close) {
          offset++;
          return;
        }
        ensure(text[offset++] === ",", "invalid JSON separator");
        whitespace();
      }
    }
    if (char === '"') {
      string();
      return;
    }
    const token = /^(?:true|false|null|-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)/.exec(
      text.slice(offset),
    );
    ensure(token, "invalid JSON value");
    offset += token[0].length;
    if (!/^(true|false|null)$/.test(token[0])) {
      const n = Number(token[0]);
      ensure(
        Number.isFinite(n) && (!Number.isInteger(n) || Number.isSafeInteger(n)),
        "unsafe JSON number",
      );
    }
  }
  value(0);
  whitespace();
  ensure(offset === text.length, "trailing JSON data");
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw new Error("invalid JSON document");
  }
}

// The policy format is a deliberately bounded YAML mapping subset. Aliases,
// tags, merge keys, flow collections and multiline scalars have no policy role.
export function policyYaml(text: string): ObjectValue {
  ensure(
    Buffer.byteLength(text) <= 65536 && !/[\t\x00-\x08\x0b-\x1f\x7f]/.test(text),
    "invalid policy YAML",
  );
  const root: ObjectValue = Object.create(null);
  const stack = [{ indent: -2, value: root }];
  for (const original of text.split("\n")) {
    let quote = "",
      comment = original.length;
    for (let i = 0; i < original.length; i++) {
      const char = original[i];
      if (quote === '"' && char === "\\") {
        i++;
        continue;
      }
      if (quote === "'" && char === "'" && original[i + 1] === "'") {
        i++;
        continue;
      }
      if (quote) {
        if (char === quote) quote = "";
      } else if (char === '"' || char === "'") quote = char;
      else if (char === "#" && (i === 0 || /\s/.test(original[i - 1]))) {
        comment = i;
        break;
      }
    }
    const line = original.slice(0, comment).trimEnd();
    if (!line.trim() || line.trimStart().startsWith("#")) continue;
    const match = /^( *)([a-z][a-z0-9_-]*):(?: +(.*))?$/.exec(line);
    ensure(match && match[1].length % 2 === 0, "unsupported policy YAML construct");
    const indent = match[1].length;
    while (stack.length > 1 && indent <= stack.at(-1)!.indent) stack.pop();
    const parent = stack.at(-1)!;
    ensure(indent === parent.indent + 2, "invalid policy indentation");
    const key = match[2];
    ensure(!Object.hasOwn(parent.value, key), "duplicate YAML member");
    const raw = match[3];
    if (raw === undefined) {
      const child: ObjectValue = Object.create(null);
      parent.value[key] = child;
      stack.push({ indent, value: child });
      continue;
    }
    let parsed: unknown;
    if (raw === "{}") parsed = Object.create(null);
    else if (raw === "true" || raw === "false") parsed = raw === "true";
    else if (raw === "null" || raw === "~") parsed = null;
    else if (/^(?:0|[1-9][0-9]*)$/.test(raw))
      parsed = integer(raw, 0, Number.MAX_SAFE_INTEGER, key);
    else if (raw.startsWith('"')) {
      parsed = strictJson(raw);
      ensure(typeof parsed === "string", "invalid YAML scalar");
    } else if (raw.startsWith("'")) {
      ensure(/^'(?:[^']|'')*'$/.test(raw), "invalid YAML scalar");
      parsed = raw.slice(1, -1).replaceAll("''", "'");
    } else {
      ensure(!/^[!&*{[>|%\-?]|:\s|\s[&*!]/.test(raw), "unsupported policy YAML scalar");
      parsed = raw;
    }
    parent.value[key] = parsed;
  }
  return root;
}
export function explicitBoolean(value: unknown, name: string, fallback?: boolean): boolean {
  const text = String(value ?? "")
    .trim()
    .toLowerCase();
  if (text === "" && fallback !== undefined) return fallback;
  if (["1", "true", "yes", "on"].includes(text)) return true;
  if (["0", "false", "no", "off"].includes(text)) return false;
  throw new Error(`invalid ${name}`);
}
export function timestamp(value: unknown, name: string): number {
  ensure(
    typeof value === "string" &&
      /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?(?:Z|[+-]\d\d:\d\d)$/.test(value),
    `invalid ${name}`,
  );
  const n = Date.parse(value);
  const day = Number(value.slice(8, 10));
  const month = Number(value.slice(5, 7));
  const days = new Date(Date.UTC(Number(value.slice(0, 4)), month, 0)).getUTCDate();
  ensure(
    Number.isFinite(n) &&
      month >= 1 &&
      month <= 12 &&
      day >= 1 &&
      day <= days &&
      Number(value.slice(11, 13)) <= 23 &&
      Number(value.slice(14, 16)) <= 59 &&
      Number(value.slice(17, 19)) <= 59,
    `invalid ${name}`,
  );
  return n;
}
export function dnsName(value: string): boolean {
  return (
    value.length <= 253 &&
    value.includes(".") &&
    value.split(".").every((label) => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label))
  );
}
export function encodeRecords(value: Record<string, string | boolean>): string {
  return Object.entries(value)
    .map(([key, item]) => `${key}=${item}\n`)
    .join("");
}
