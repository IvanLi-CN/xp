export const DEMO_SHARE_KEYS = [
	"demo",
	"d_persona",
	"d_network",
	"d_profile",
	"d_format",
	"d_mirror",
	"d_preview",
] as const;

export function buildDemoShareUrl(href: string): string {
	const url = new URL(href, "http://demo.invalid");
	const search = new URLSearchParams();
	for (const key of DEMO_SHARE_KEYS) {
		const value = url.searchParams.get(key);
		if (value !== null) search.set(key, value);
	}
	url.search = search.toString();
	url.hash = "";
	return url.origin === "http://demo.invalid"
		? `${url.pathname}${url.search}`
		: url.toString();
}

export type DemoControls = {
	scene: "normal" | "incident" | "empty" | "large";
	persona: "admin" | "anonymous";
	network: "online" | "slow" | "offline" | "error";
	profile: "valid" | "missing" | "invalid";
};

export const DEFAULT_DEMO_CONTROLS: DemoControls = {
	scene: "normal",
	persona: "admin",
	network: "online",
	profile: "valid",
};

function choice<T extends string>(
	value: string | null,
	options: readonly T[],
	fallback: T,
): T {
	return options.find((option) => option === value) ?? fallback;
}

export function readDemoControls(search: string): DemoControls {
	const params = new URLSearchParams(search);
	return {
		scene: choice(
			params.get("demo"),
			["normal", "incident", "empty", "large"],
			"normal",
		),
		persona: choice(params.get("d_persona"), ["admin", "anonymous"], "admin"),
		network: choice(
			params.get("d_network"),
			["online", "slow", "offline", "error"],
			"online",
		),
		profile: choice(
			params.get("d_profile"),
			["valid", "missing", "invalid"],
			"valid",
		),
	};
}

export function demoControlSearch(controls: DemoControls) {
	return {
		demo: controls.scene,
		d_persona: controls.persona,
		d_network: controls.network,
		d_profile: controls.profile,
	};
}
