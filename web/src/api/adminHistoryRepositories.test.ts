import { expect, it } from "vitest";

import { HistoryRepositoryRuntimeSchema } from "./adminHistoryRepositories";

it("accepts a committed SQLite runtime with degraded maintenance", () => {
	const result = HistoryRepositoryRuntimeSchema.safeParse({
		storage_mode: "sqlite_degraded",
		capacity: {
			quota_bytes: 10 * 1024 ** 3,
			used_bytes: 1024,
			filesystem_available_bytes: 1024 ** 3,
		},
		record_count: 1,
		segment_count: 1,
		gap_count: 0,
		history_truncated: false,
		last_verified_unix_seconds: null,
		last_anti_entropy_unix_seconds: null,
		last_deep_verification_unix_seconds: null,
		last_dynamic_relay_attempt_unix_seconds: null,
	});

	expect(result.success).toBe(true);
});
