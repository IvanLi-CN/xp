import { BackendApiError } from "@/api/backendError";
import { describe, expect, it } from "vitest";

import {
	beginUpgradeObservation,
	classifyUpgradeStartError,
	observeUpgradeStatus,
	refreshTimedOutObservation,
	restoreUpgradeObservation,
} from "./upgradeObservation";

const NOW = 1_700_000_000_000;
const TIMEOUT_DESCRIPTION =
	"times out after one minute and remains locked until a manual status result resolves it";

describe("upgrade observation", () => {
	it("keeps polling after an unstructured 502 from start until status becomes terminal", () => {
		const observation = beginUpgradeObservation("v3.21.10", NOW);

		expect(
			classifyUpgradeStartError(
				new BackendApiError({ status: 502, message: "request failed: 502" }),
			),
		).toBe("observe");
		expect(
			observeUpgradeStatus(
				observation,
				{ state: "idle", target_tag: null },
				NOW,
			),
		).toEqual(observation);
		expect(
			observeUpgradeStatus(
				observation,
				{ state: "succeeded", target_tag: "v3.21.10" },
				NOW + 2_500,
			),
		).toMatchObject({ phase: "terminal" });
	});

	it("ignores a stale terminal from the same start second", () => {
		const observation = beginUpgradeObservation("v3.21.10", NOW + 500);
		const stale = observeUpgradeStatus(
			observation,
			{
				state: "failed",
				target_tag: "v3.21.10",
				updated_at: new Date(NOW).toISOString(),
			},
			NOW + 2_500,
		);

		expect(stale).toEqual(observation);
		expect(
			observeUpgradeStatus(
				stale,
				{
					state: "succeeded",
					target_tag: "v3.21.10",
					updated_at: new Date(NOW + 1_000).toISOString(),
				},
				NOW + 2_500,
			),
		).toMatchObject({ phase: "terminal" });
	});

	it("accepts a terminal status after observing the current active job", () => {
		const observation = beginUpgradeObservation("v3.21.10", NOW + 500);
		const active = observeUpgradeStatus(
			observation,
			{
				state: "running",
				target_tag: "v3.21.10",
				updated_at: new Date(NOW).toISOString(),
			},
			NOW + 2_500,
		);

		expect(active).toMatchObject({ hasSeenActive: true });
		expect(
			observeUpgradeStatus(
				active,
				{
					state: "succeeded",
					target_tag: "v3.21.10",
					updated_at: new Date(NOW).toISOString(),
				},
				NOW + 2_500,
			),
		).toMatchObject({ phase: "terminal" });
	});

	it("stops polling and holds an unsupported terminal status", () => {
		expect(
			observeUpgradeStatus(
				beginUpgradeObservation("v3.21.10", NOW),
				{ state: "unsupported", target_tag: null },
				NOW + 2_500,
			),
		).toMatchObject({ phase: "terminal" });
	});

	it("accepts a terminal status at the deadline before timing out", () => {
		expect(
			observeUpgradeStatus(
				beginUpgradeObservation("v3.21.10", NOW),
				{ state: "succeeded", target_tag: "v3.21.10" },
				NOW + 60_000,
			),
		).toMatchObject({ phase: "terminal" });
	});

	it(TIMEOUT_DESCRIPTION, () => {
		const observation = beginUpgradeObservation("v3.21.10", NOW);
		const timedOut = observeUpgradeStatus(observation, null, NOW + 60_000);

		expect(timedOut).toMatchObject({ phase: "timed_out" });
		expect(
			refreshTimedOutObservation(
				timedOut,
				{ state: "idle", target_tag: null },
				NOW + 60_001,
			),
		).toBeNull();
	});

	it("opens a fresh one-minute window when manual status finds an active upgrade", () => {
		const timedOut = observeUpgradeStatus(
			beginUpgradeObservation("v3.21.10", NOW),
			null,
			NOW + 60_000,
		);
		const resumed = refreshTimedOutObservation(
			timedOut,
			{ state: "restarting", target_tag: "v3.21.10" },
			NOW + 60_001,
		);

		expect(resumed).toMatchObject({
			phase: "observing",
			targetTag: "v3.21.10",
			deadlineAtMs: NOW + 120_001,
		});
	});

	it("restores only the remaining observation window from same-tab storage", () => {
		expect(
			restoreUpgradeObservation(
				JSON.stringify({
					targetTag: "v3.21.10",
					deadlineAtMs: NOW + 12_500,
					phase: "observing",
				}),
				NOW,
			),
		).toMatchObject({ phase: "observing", deadlineAtMs: NOW + 12_500 });
		expect(
			restoreUpgradeObservation(
				JSON.stringify({
					targetTag: "v3.21.10",
					deadlineAtMs: NOW - 1,
					phase: "observing",
				}),
				NOW,
			),
		).toMatchObject({ phase: "timed_out" });
	});

	it("preserves a stored terminal result after its original deadline", () => {
		expect(
			restoreUpgradeObservation(
				JSON.stringify({
					targetTag: "v3.21.10",
					deadlineAtMs: NOW - 1,
					phase: "terminal",
				}),
				NOW,
			),
		).toMatchObject({ phase: "terminal" });
	});

	it("treats structured rejections as final and an existing active job as observable", () => {
		expect(
			classifyUpgradeStartError(
				new BackendApiError({
					status: 409,
					code: "upgrade_already_running",
					message: "upgrade already running",
				}),
			),
		).toBe("observe_existing");
		expect(
			classifyUpgradeStartError(
				new BackendApiError({
					status: 503,
					code: "upgrade_unsupported",
					message: "unsupported",
				}),
			),
		).toBe("failed");
	});
});
