import { describe, expect, it } from "vitest";

import { resourceQueryRefetchInterval } from "./useNodeResourceQueries";

describe("resource query polling", () => {
	it("pauses errored or offline queries and resumes after success", () => {
		expect(resourceQueryRefetchInterval(true, "error", 15_000)).toBe(false);
		expect(resourceQueryRefetchInterval(false, "success", 15_000)).toBe(false);
		expect(resourceQueryRefetchInterval(true, "success", 15_000)).toBe(15_000);
		expect(resourceQueryRefetchInterval(true, "pending", 30_000)).toBe(30_000);
	});
});
