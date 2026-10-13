import { describe, expect, it } from "vitest";

import { buildDemoShareUrl } from "./shareState";

describe("demo share state", () => {
	it("keeps only explicit demo state and the current page", () => {
		const shared = buildDemoShareUrl(
			"https://user:secret@xp.example/demo/users/user-fixture-c?demo=normal&d_persona=admin" +
				"&login_token=secret&payload=drop#secret",
		);

		expect(shared).toBe(
			"https://xp.example/demo/users/user-fixture-c?demo=normal&d_persona=admin",
		);
	});
});
