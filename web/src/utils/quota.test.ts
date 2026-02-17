import { describe, expect, it } from "vitest";

import {
	formatQuotaBytesCompactInput,
	formatQuotaBytesHuman,
	parseQuotaInputToBytes,
} from "./quota";

describe("quota utils", () => {
	describe("parseQuotaInputToBytes", () => {
		it("parses GiB variants", () => {
			expect(parseQuotaInputToBytes("10GiB")).toEqual({
				ok: true,
				bytes: 10 * 2 ** 30,
			});
			expect(parseQuotaInputToBytes("10G")).toEqual({
				ok: true,
				bytes: 10 * 2 ** 30,
			});
			expect(parseQuotaInputToBytes("10gi")).toEqual({
				ok: true,
				bytes: 10 * 2 ** 30,
			});
			expect(parseQuotaInputToBytes("10 gib")).toEqual({
				ok: true,
				bytes: 10 * 2 ** 30,
			});
			expect(parseQuotaInputToBytes("10 GiByte")).toEqual({
				ok: true,
				bytes: 10 * 2 ** 30,
			});
			expect(parseQuotaInputToBytes("10 gibibyte")).toEqual({
				ok: true,
				bytes: 10 * 2 ** 30,
			});
		});

		it("parses MiB variants", () => {
			expect(parseQuotaInputToBytes("512MiB")).toEqual({
				ok: true,
				bytes: 512 * 2 ** 20,
			});
			expect(parseQuotaInputToBytes("512M")).toEqual({
				ok: true,
				bytes: 512 * 2 ** 20,
			});
			expect(parseQuotaInputToBytes("512mi")).toEqual({
				ok: true,
				bytes: 512 * 2 ** 20,
			});
			expect(parseQuotaInputToBytes("512 mib")).toEqual({
				ok: true,
				bytes: 512 * 2 ** 20,
			});
			expect(parseQuotaInputToBytes("512 MiByte")).toEqual({
				ok: true,
				bytes: 512 * 2 ** 20,
			});
			expect(parseQuotaInputToBytes("512 mebibyte")).toEqual({
				ok: true,
				bytes: 512 * 2 ** 20,
			});
		});

		it("defaults to MiB when unit is omitted", () => {
			expect(parseQuotaInputToBytes("10")).toEqual({
				ok: true,
				bytes: 10 * 2 ** 20,
			});
		});

		it("supports decimals with rounding", () => {
			expect(parseQuotaInputToBytes("1.5GiB")).toEqual({
				ok: true,
				bytes: Math.round(1.5 * 2 ** 30),
			});
		});

		it("treats GB/MB as GiB/MiB", () => {
			expect(parseQuotaInputToBytes("10GB")).toEqual({
				ok: true,
				bytes: 10 * 2 ** 30,
			});
			expect(parseQuotaInputToBytes("10MB")).toEqual({
				ok: true,
				bytes: 10 * 2 ** 20,
			});
		});

		it("parses TiB/PiB and treats TB/PB as TiB/PiB", () => {
			expect(parseQuotaInputToBytes("2TiB")).toEqual({
				ok: true,
				bytes: 2 * 2 ** 40,
			});
			expect(parseQuotaInputToBytes("2TB")).toEqual({
				ok: true,
				bytes: 2 * 2 ** 40,
			});
			expect(parseQuotaInputToBytes("1PiB")).toEqual({
				ok: true,
				bytes: 1 * 2 ** 50,
			});
			expect(parseQuotaInputToBytes("1PB")).toEqual({
				ok: true,
				bytes: 1 * 2 ** 50,
			});
		});

		it("rejects empty and invalid input", () => {
			expect(parseQuotaInputToBytes("")).toEqual({
				ok: false,
				error: "Quota is required.",
			});
			expect(parseQuotaInputToBytes("abc")).toEqual({
				ok: false,
				error: "Invalid quota format.",
			});
			expect(parseQuotaInputToBytes("-1GiB")).toEqual({
				ok: false,
				error: "Quota must be zero or greater.",
			});
			expect(parseQuotaInputToBytes("10XB")).toEqual({
				ok: false,
				error:
					"Unsupported unit (use MiB/GiB/TiB/PiB; also accepts M/G/T/P, MB/GB/TB/PB).",
			});
		});

		it("rejects values above Number.MAX_SAFE_INTEGER", () => {
			// A large enough GiB value will exceed safe integer bytes.
			const res = parseQuotaInputToBytes("99999999999GiB");
			expect(res.ok).toBe(false);
		});
	});

	describe("formatters", () => {
		it("formats bytes to human readable", () => {
			expect(formatQuotaBytesHuman(0)).toBe("0");
			expect(formatQuotaBytesHuman(1 * 2 ** 50)).toBe("1 PiB");
			expect(formatQuotaBytesHuman(2 * 2 ** 40)).toBe("2 TiB");
			expect(formatQuotaBytesHuman(10 * 2 ** 30)).toBe("10 GiB");
			expect(formatQuotaBytesHuman(512 * 2 ** 20)).toBe("512 MiB");
		});

		it("formats compact input", () => {
			expect(formatQuotaBytesCompactInput(0)).toBe("0");
			expect(formatQuotaBytesCompactInput(1 * 2 ** 50)).toBe("1PiB");
			expect(formatQuotaBytesCompactInput(2 * 2 ** 40)).toBe("2TiB");
			expect(formatQuotaBytesCompactInput(10 * 2 ** 30)).toBe("10GiB");
			expect(formatQuotaBytesCompactInput(512 * 2 ** 20)).toBe("512MiB");
		});
	});
});
