import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useTimeWindowTransition } from "./useTimeWindowTransition";

type Window = "24h" | "7d";

const alignData = (data: string, window: Window) => `${data}:${window}`;
const createEmptyData = (_data: string, window: Window) => `empty:${window}`;

type HookProps = {
	data: string | undefined;
	dataUpdatedAt: number;
	isError: boolean;
	isFetchedAfterMount?: boolean;
	isFetching: boolean;
	window: Window;
};

function useTransition(props: HookProps) {
	return useTimeWindowTransition({
		...props,
		alignData,
		createEmptyData,
		holdMs: 300,
		identity: "report",
		isFetchedAfterMount: props.isFetchedAfterMount ?? true,
		now: () => 1_000,
	});
}

describe("useTimeWindowTransition", () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it("holds the previous report for 300ms, then shows an aligned empty target", () => {
		const { result, rerender } = renderHook(useTransition, {
			initialProps: {
				data: "cached",
				dataUpdatedAt: 1,
				isError: false,
				isFetching: false,
				window: "24h",
			},
		});

		rerender({
			data: undefined,
			dataUpdatedAt: 0,
			isError: false,
			isFetching: true,
			window: "7d",
		});
		expect(result.current.data).toBe("cached:24h");
		expect(result.current.isWindowPending).toBe(true);

		act(() => vi.advanceTimersByTime(299));
		expect(result.current.data).toBe("cached:24h");

		act(() => vi.advanceTimersByTime(1));
		expect(result.current.data).toBe("empty:7d");
		expect(result.current.displayWindow).toBe("7d");
		expect(result.current.isWindowPending).toBe(true);
	});

	it("commits a fresh target response before the hold expires", () => {
		const { result, rerender } = renderHook(useTransition, {
			initialProps: {
				data: "cached",
				dataUpdatedAt: 1,
				isError: false,
				isFetching: false,
				window: "24h",
			},
		});

		rerender({
			data: undefined,
			dataUpdatedAt: 0,
			isError: false,
			isFetching: true,
			window: "7d",
		});
		rerender({
			data: "fresh",
			dataUpdatedAt: 2,
			isError: false,
			isFetching: false,
			window: "7d",
		});

		expect(result.current.data).toBe("fresh");
		expect(result.current.isWindowPending).toBe(false);
		act(() => vi.advanceTimersByTime(300));
		expect(result.current.data).toBe("fresh");
	});

	it("aligns persisted data restored asynchronously after mount", () => {
		const { result, rerender } = renderHook(useTransition, {
			initialProps: {
				data: undefined,
				dataUpdatedAt: 0,
				isError: false,
				isFetchedAfterMount: false,
				isFetching: false,
				window: "24h",
			},
		});

		rerender({
			data: "restored-cache",
			dataUpdatedAt: 1,
			isError: false,
			isFetchedAfterMount: false,
			isFetching: false,
			window: "24h",
		});

		expect(result.current.data).toBe("restored-cache:24h");
	});

	it("shows the aligned target cache after the hold expires", () => {
		const { result, rerender } = renderHook(useTransition, {
			initialProps: {
				data: "current",
				dataUpdatedAt: 2,
				isError: false,
				isFetching: false,
				window: "24h",
			},
		});

		rerender({
			data: "target-cache",
			dataUpdatedAt: 1,
			isError: false,
			isFetching: true,
			window: "7d",
		});
		expect(result.current.data).toBe("current:24h");
		act(() => vi.advanceTimersByTime(300));
		expect(result.current.data).toBe("target-cache:7d");
		expect(result.current.isWindowPending).toBe(true);
	});

	it("stops the overlay after a failed refresh while preserving fallback data", () => {
		const { result, rerender } = renderHook(useTransition, {
			initialProps: {
				data: "current",
				dataUpdatedAt: 2,
				isError: false,
				isFetching: false,
				window: "24h",
			},
		});

		rerender({
			data: undefined,
			dataUpdatedAt: 0,
			isError: false,
			isFetching: true,
			window: "7d",
		});
		act(() => vi.advanceTimersByTime(300));
		rerender({
			data: undefined,
			dataUpdatedAt: 0,
			isError: true,
			isFetching: false,
			window: "7d",
		});

		expect(result.current.data).toBe("empty:7d");
		expect(result.current.isWindowPending).toBe(false);
	});

	it("ignores an obsolete timer after another window selection", () => {
		const { result, rerender } = renderHook(useTransition, {
			initialProps: {
				data: "cached",
				dataUpdatedAt: 1,
				isError: false,
				isFetching: false,
				window: "24h",
			},
		});

		rerender({
			data: undefined,
			dataUpdatedAt: 0,
			isError: false,
			isFetching: true,
			window: "7d",
		});
		act(() => vi.advanceTimersByTime(100));
		rerender({
			data: "latest",
			dataUpdatedAt: 3,
			isError: false,
			isFetching: true,
			window: "24h",
		});
		act(() => vi.advanceTimersByTime(300));

		expect(result.current.data).toBe("latest:24h");
		expect(result.current.displayWindow).toBe("24h");
	});
});
