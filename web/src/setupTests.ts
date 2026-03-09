import "@testing-library/jest-dom/vitest";

if (!HTMLElement.prototype.scrollIntoView) {
	HTMLElement.prototype.scrollIntoView = () => {};
}

if (typeof globalThis.ResizeObserver === "undefined") {
	globalThis.ResizeObserver = class {
		disconnect() {}
		observe() {}
		unobserve() {}
	} as typeof ResizeObserver;
}
