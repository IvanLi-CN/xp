import { describe, expect, it } from "vitest";

import { transformIndexHtmlWithInlineBuildDeclaration } from "./runtime/inlineBuildDeclaration";

describe("Vite inline build declaration", () => {
	it("keeps adversarial build IDs inside the transformed script", () => {
		const maliciousBuildId = '</script><img src=x onerror="alert(1)">';
		const html = transformIndexHtmlWithInlineBuildDeclaration(
			'<html><head></head><body><script type="module" src="/src/main.tsx"></script></body></html>',
			maliciousBuildId,
		);

		expect(html).not.toContain("</script><img");
		expect(html).toContain("\\u003c/script\\u003e");
		expect(html).toContain(
			"?xp-build=%3C%2Fscript%3E%3Cimg%20src%3Dx%20onerror%3D%22alert(1)%22%3E",
		);
	});
});
