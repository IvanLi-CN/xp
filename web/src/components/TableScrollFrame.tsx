import {
	type ReactNode,
	useCallback,
	useEffect,
	useRef,
	useState,
} from "react";

import { cn } from "@/lib/utils";

import { Icon } from "./Icon";

type TableScrollFrameProps = {
	children: ReactNode;
	ariaLabel?: string;
	className?: string;
};

export function TableScrollFrame({
	children,
	ariaLabel = "Data table",
	className,
}: TableScrollFrameProps) {
	const frameRef = useRef<HTMLDivElement>(null);
	const [isOverflowing, setIsOverflowing] = useState(false);

	const measureOverflow = useCallback(() => {
		const frame = frameRef.current;
		if (!frame) return;
		setIsOverflowing(frame.scrollWidth > frame.clientWidth + 1);
	}, []);

	useEffect(() => {
		measureOverflow();
		const frame = frameRef.current;
		if (!frame) return;

		const resizeObserver =
			typeof ResizeObserver === "undefined"
				? null
				: new ResizeObserver(measureOverflow);
		resizeObserver?.observe(frame);
		window.addEventListener("resize", measureOverflow);

		return () => {
			resizeObserver?.disconnect();
			window.removeEventListener("resize", measureOverflow);
		};
	}, [measureOverflow]);

	return (
		<div
			ref={frameRef}
			className={cn("xp-table-wrap", className)}
			data-overflowing={isOverflowing}
			onScroll={measureOverflow}
			role={isOverflowing ? "region" : undefined}
			tabIndex={isOverflowing ? 0 : undefined}
			aria-label={isOverflowing ? ariaLabel : undefined}
		>
			{isOverflowing ? (
				<div className="xp-table-scroll-hint" aria-hidden="true">
					<Icon name="tabler:arrows-horizontal" size={15} />
					<span>More columns</span>
				</div>
			) : null}
			{children}
		</div>
	);
}
