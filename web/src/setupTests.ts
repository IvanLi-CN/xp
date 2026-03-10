import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(() => cleanup());

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
