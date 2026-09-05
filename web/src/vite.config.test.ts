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

	it("pins the entry script and stylesheet to the declared build", () => {
		const html = transformIndexHtmlWithInlineBuildDeclaration(
			[
				"<html><head>",
				'<link rel="stylesheet" crossorigin href="/assets/index.css">',
				"</head><body>",
				'<script type="module" src="/src/main.tsx"></script>',
				"</body></html>",
			].join(""),
			"2026.09.05-build",
		);

		expect(html).toContain(
			'href="/assets/index.css?xp-build=2026.09.05-build"',
		);
		expect(html).toContain('src="/src/main.tsx?xp-build=2026.09.05-build"');
	});

	it("preserves an existing stylesheet query when pinning a build", () => {
		const html = transformIndexHtmlWithInlineBuildDeclaration(
			'<html><head><link rel="stylesheet" href="/assets/index.css?theme=dark"></head></html>',
			"next",
		);

		expect(html).toContain('href="/assets/index.css?theme=dark&xp-build=next"');
	});
});
