import { createHash } from "node:crypto";
import { ensure, integer, object, records, strictJson, type ObjectValue } from "./input.ts";

export function strictBase64(value: string): Buffer {
  ensure(
    /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value),
    "invalid base64",
  );
  const bytes = Buffer.from(value, "base64");
  ensure(bytes.toString("base64") === value, "noncanonical base64");
  return bytes;
}
type Field =
  | { number: number; wire: 0; value: bigint }
  | { number: number; wire: 2; value: Buffer };
function protobuf(bytes: Buffer): Field[] {
  let offset = 0;
  const varint = (): bigint => {
    let result = 0n;
    for (let shift = 0n; shift <= 63n; shift += 7n) {
      ensure(offset < bytes.length, "truncated Xray protobuf");
      const byte = bytes[offset++];
      ensure(shift !== 63n || byte <= 1, "overflowed Xray protobuf");
      result |= BigInt(byte & 127) << shift;
      if (byte < 128) return result;
    }
    throw new Error("invalid Xray protobuf");
  };
  const result: Field[] = [];
  while (offset < bytes.length) {
    const key = varint(),
      number = Number(key >> 3n),
      wire = Number(key & 7n);
    ensure(number >= 1 && number <= 536870911, "invalid protobuf field");
    if (wire === 0) result.push({ number, wire, value: varint() });
    else {
      ensure(wire === 2, "unsupported protobuf wire type");
      const length = varint();
      ensure(length <= BigInt(bytes.length - offset), "truncated protobuf field");
      result.push({ number, wire, value: bytes.subarray(offset, offset + Number(length)) });
      offset += Number(length);
    }
  }
  return result;
}
function singleField(fields: Field[], number: number, wire: 0): bigint;
function singleField(fields: Field[], number: number, wire: 2): Buffer;
function singleField(fields: Field[], number: number, wire: 0 | 2): bigint | Buffer {
  const selected = fields.filter((field) => field.number === number);
  ensure(selected.length === 1 && selected[0].wire === wire, "ambiguous Xray protobuf field");
  return selected[0].value;
}
export function inboundPort(inbound: ObjectValue): number {
  ensure(!(inbound.receiverSettings && inbound.receiver_settings), "ambiguous receiver settings");
  const receiver = object(
    inbound.receiverSettings ?? inbound.receiver_settings,
    "receiver settings",
  );
  ensure(
    (receiver.type ?? receiver._TypedMessage_) === "xray.app.proxyman.ReceiverConfig",
    "invalid receiver type",
  );
  if (receiver.portList !== undefined) {
    ensure(typeof receiver.portList === "number", "invalid receiver port");
    return integer(String(receiver.portList), 1, 65535, "receiver port");
  }
  ensure(
    typeof receiver.value === "string" && receiver.value.length > 0,
    "missing receiver protobuf",
  );
  const portList = singleField(protobuf(strictBase64(receiver.value)), 1, 2);
  const range = singleField(protobuf(portList), 1, 2);
  const fields = protobuf(range),
    from = singleField(fields, 1, 0),
    to = singleField(fields, 2, 0);
  ensure(from === to, "receiver range must contain one port");
  return integer(String(from), 1, 65535, "receiver port");
}
function list(value: string, max: number, name: string): number[] {
  const items = value === "" ? [] : value.split(",").map((item) => integer(item, 1, max, name));
  ensure(new Set(items).size === items.length, `duplicate ${name}`);
  return items;
}
export function preflight(text: string, host: string): number {
  const r = records(text, [
    "runtime_json",
    "state_b64",
    "occupied_ports",
    "ingress_ports",
    "ingress_listener_pids",
    "ephemeral_range",
    "reserved_ports",
    "unprivileged_port_start",
    "ingress_runtime_pid",
    "ingress_service",
    "ingress_security",
    "ingress_api_b64",
  ]);
  const runtime = object(strictJson(r.runtime_json), "runtime state");
  ensure(
    ["xray", "xp-proxy"].includes(String(runtime.runtime)) &&
      runtime.state === "active" &&
      runtime.error == null,
    "ingress runtime not active",
  );
  const state = object(
    strictJson(new TextDecoder("utf-8", { fatal: true }).decode(strictBase64(r.state_b64))),
    "managed state",
  );
  if (Object.keys(state).length) {
    ensure(
      state.schema_version === 1 && Object.hasOwn(state, "vless_endpoint_id"),
      "unsupported managed state schema",
    );
    ensure(
      state.vless_endpoint_id === null ||
        (typeof state.vless_endpoint_id === "string" && state.vless_endpoint_id.length > 0),
      "invalid endpoint identity",
    );
  }
  const xray = list(r.ingress_ports, 65535, "Xray port"),
    pids = list(r.ingress_listener_pids, Number.MAX_SAFE_INTEGER, "listener PID");
  ensure(
    r.ingress_runtime_pid === "" || /^[1-9][0-9]*$/.test(r.ingress_runtime_pid),
    "invalid Xray runtime PID",
  );
  ensure(["active", "inactive"].includes(r.ingress_service), "invalid Xray service");
  if (state.vless_endpoint_id != null) {
    ensure(
      xray.length === 1 &&
        pids.length === 1 &&
        String(pids[0]) === r.ingress_runtime_pid &&
        r.ingress_service === "active",
      "managed endpoint requires one verified listener process",
    );
    const api = strictJson(
      new TextDecoder("utf-8", { fatal: true }).decode(strictBase64(r.ingress_api_b64)),
    );
    if (runtime.runtime === "xp-proxy") {
      const snapshot = object(api, "proxy snapshot");
      ensure(
        Number.isSafeInteger(snapshot.generation) && Number(snapshot.generation) > 0,
        "missing proxy generation",
      );
      const listeners = snapshot.listeners;
      ensure(Array.isArray(listeners), "invalid proxy listeners");
      const selected = listeners;
      ensure(selected.length === 1, "managed proxy listener missing or ambiguous");
      const bind = String(object(selected[0], "proxy listener").bind);
      ensure(
        /^(?:0\.0\.0\.0|\[::\]):[1-9][0-9]*$/.test(bind) &&
          Number(bind.slice(bind.lastIndexOf(":") + 1)) === xray[0],
        "proxy listener port mismatch",
      );
      return xray[0];
    }
    const inbounds = Array.isArray(api) ? api : object(api, "Xray inventory").inbounds;
    ensure(Array.isArray(inbounds), "invalid Xray inventory");
    const selected = inbounds.filter(
      (item) =>
        item && typeof item === "object" && item.tag === `vless-vision-${state.vless_endpoint_id}`,
    );
    ensure(
      selected.length === 1 && inboundPort(object(selected[0], "inbound")) === xray[0],
      "listener is not owned by managed inbound",
    );
    return xray[0];
  }
  ensure(xray.length === 0, "public listener exists without managed endpoint state");
  const unavailable = new Set(list(r.occupied_ports, 65535, "occupied port"));
  const bounds = r.ephemeral_range.trim().split(/\s+/);
  ensure(bounds.length === 2, "invalid ephemeral range");
  function reserve(first: string, last: string): void {
    const start = integer(first, 1, 65535, "port range"),
      end = integer(last, start, 65535, "port range");
    for (let port = start; port <= end; port++) unavailable.add(port);
  }
  reserve(bounds[0], bounds[1]);
  if (r.reserved_ports)
    for (const entry of r.reserved_ports.split(",")) {
      const pair = entry.split("-");
      ensure(pair.length <= 2, "invalid reserved range");
      reserve(pair[0], pair[1] ?? pair[0]);
    }
  const unprivileged = integer(r.unprivileged_port_start, 0, 65535, "unprivileged port start");
  let privileged = false;
  if (r.ingress_security !== "unavailable") {
    const match = /^(0|[1-9][0-9]*) ([0-9a-fA-F]{1,16})$/.exec(r.ingress_security);
    ensure(match, "invalid Xray security record");
    privileged = (BigInt(`0x${match[2]}`) & 1024n) !== 0n;
  }
  const candidates: number[] = [];
  for (let port = privileged ? 1 : Math.max(unprivileged, 1); port <= 65535; port++)
    if (!unavailable.has(port)) candidates.push(port);
  ensure(candidates.length > 0, "no bindable VLESS bootstrap port");
  return candidates[
    Number.parseInt(createHash("sha256").update(host).digest("hex").slice(0, 8), 16) %
      candidates.length
  ];
}
export function composeImageMatches(text: string, expected: string): boolean {
  const images = text.split(/\r?\n/).filter((line) => line.trim() !== "");
  return (
    expected.length > 0 &&
    !/[\r\n\x00]/.test(expected) &&
    images.length === 1 &&
    images[0] === expected
  );
}
