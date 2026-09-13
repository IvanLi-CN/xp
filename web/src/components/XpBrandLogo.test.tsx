import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import iconAssets from "../../assets-src/icon-assets.json";
import { XpBrandLogo } from "./XpBrandLogo";

describe("<XpBrandLogo />", () => {
	it("uses the generated bicolor mark for compact surfaces", () => {
		render(<XpBrandLogo kind="mark" />);

		expect(screen.getByAltText("xp")).toHaveAttribute(
			"src",
			`/${iconAssets.bicolorSvg}`,
		);
	});

	it("keeps light and inverse lockups available for theme switching", () => {
		render(<XpBrandLogo kind="lockup" />);

		const logos = screen.getAllByAltText("xp");
		expect(logos).toHaveLength(2);
		expect(logos[0]).toHaveClass("dark:hidden");
		expect(logos[1]).toHaveClass("dark:block");
		expect(logos[0]).toHaveAttribute("src", `/${iconAssets.lockupSvg}`);
		expect(logos[1]).toHaveAttribute("src", `/${iconAssets.lockupInverseSvg}`);
	});
});
