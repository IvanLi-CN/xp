import { useCallback, useLayoutEffect, useRef, useState } from "react";

export const TIME_WINDOW_HOLD_MS = 300;

type DisplayState<T, W> = {
	data: T;
	dataUpdatedAt: number;
	window: W;
};

type TransitionPhase = "idle" | "holding" | "loading";

type UseTimeWindowTransitionOptions<T, W> = {
	alignData: (data: T, window: W, now: number) => T;
	createEmptyData: (previousData: T, window: W, now: number) => T;
	data: T | undefined;
	dataUpdatedAt: number;
	holdMs?: number;
	identity: string;
	isError: boolean;
	isFetchedAfterMount: boolean;
	isFetching: boolean;
	now?: () => number;
	window: W;
};

type ActiveTransition<T, W> = {
	holdExpired: boolean;
	initialData: T | undefined;
	initialDataUpdatedAt: number;
	window: W;
};

export function useTimeWindowTransition<T, W>({
	alignData,
	createEmptyData,
	data,
	dataUpdatedAt,
	holdMs = TIME_WINDOW_HOLD_MS,
	identity,
	isError,
	isFetchedAfterMount,
	isFetching,
	now = Date.now,
	window,
}: UseTimeWindowTransitionOptions<T, W>) {
	const initialNow = now();
	const [display, setDisplayState] = useState<DisplayState<T, W> | null>(() =>
		data
			? {
					data: alignData(data, window, initialNow),
					dataUpdatedAt,
					window,
				}
			: null,
	);
	const [phase, setPhase] = useState<TransitionPhase>("idle");
	const displayRef = useRef(display);
	const identityRef = useRef(identity);
	const selectedWindowRef = useRef(window);
	const transitionRef = useRef<ActiveTransition<T, W> | null>(null);
	const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
	const latestRef = useRef({
		alignData,
		createEmptyData,
		data,
		dataUpdatedAt,
		isError,
		isFetchedAfterMount,
		isFetching,
		now,
	});
	latestRef.current = {
		alignData,
		createEmptyData,
		data,
		dataUpdatedAt,
		isError,
		isFetchedAfterMount,
		isFetching,
		now,
	};

	const setDisplay = useCallback((next: DisplayState<T, W> | null) => {
		displayRef.current = next;
		setDisplayState(next);
	}, []);

	const clearTimer = useCallback(() => {
		if (timerRef.current === null) return;
		clearTimeout(timerRef.current);
		timerRef.current = null;
	}, []);

	useLayoutEffect(() => {
		if (identityRef.current !== identity) {
			clearTimer();
			const latest = latestRef.current;
			identityRef.current = identity;
			selectedWindowRef.current = window;
			transitionRef.current = null;
			setPhase("idle");
			setDisplay(
				latest.data
					? {
							data: latest.alignData(latest.data, window, latest.now()),
							dataUpdatedAt: latest.dataUpdatedAt,
							window,
						}
					: null,
			);
			return;
		}

		if (selectedWindowRef.current === window) return;
		selectedWindowRef.current = window;
		clearTimer();

		const previousDisplay = displayRef.current;
		if (!previousDisplay) {
			transitionRef.current = null;
			setPhase("idle");
			return;
		}

		const transition: ActiveTransition<T, W> = {
			holdExpired: false,
			initialData: latestRef.current.data,
			initialDataUpdatedAt: latestRef.current.dataUpdatedAt,
			window,
		};
		transitionRef.current = transition;
		setPhase("holding");
		timerRef.current = setTimeout(() => {
			const active = transitionRef.current;
			if (!active || active !== transition || active.window !== window) return;
			active.holdExpired = true;
			const latest = latestRef.current;
			const nextData = latest.data
				? latest.alignData(latest.data, window, latest.now())
				: latest.createEmptyData(previousDisplay.data, window, latest.now());
			setDisplay({
				data: nextData,
				dataUpdatedAt: latest.dataUpdatedAt,
				window,
			});
			if (latest.isFetching) {
				setPhase("loading");
				return;
			}
			transitionRef.current = null;
			setPhase("idle");
		}, holdMs);

		return clearTimer;
	}, [clearTimer, holdMs, identity, setDisplay, window]);

	useLayoutEffect(() => {
		const transition = transitionRef.current;
		if (!transition || transition.window !== window) {
			if (
				data &&
				selectedWindowRef.current === window &&
				(!displayRef.current ||
					dataUpdatedAt > displayRef.current.dataUpdatedAt)
			) {
				setDisplay({
					data: isFetchedAfterMount ? data : alignData(data, window, now()),
					dataUpdatedAt,
					window,
				});
			}
			return;
		}

		const receivedFreshData =
			data !== undefined &&
			(data !== transition.initialData ||
				dataUpdatedAt > transition.initialDataUpdatedAt);
		if (receivedFreshData) {
			clearTimer();
			transitionRef.current = null;
			setDisplay({
				data: isFetchedAfterMount ? data : alignData(data, window, now()),
				dataUpdatedAt,
				window,
			});
			setPhase("idle");
			return;
		}

		if (isError && transition.holdExpired) {
			transitionRef.current = null;
			setPhase("idle");
		}
	}, [
		alignData,
		clearTimer,
		data,
		dataUpdatedAt,
		isError,
		isFetchedAfterMount,
		now,
		setDisplay,
		window,
	]);

	useLayoutEffect(() => clearTimer, [clearTimer]);

	return {
		data: display?.data,
		displayWindow: display?.window,
		isWindowPending: phase !== "idle",
	};
}
