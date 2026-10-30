import * as SliderPrimitive from "@radix-ui/react-slider";
import * as React from "react";

import { cn } from "@/lib/utils";

type SliderProps = React.ComponentPropsWithoutRef<
	typeof SliderPrimitive.Root
> & {
	label: string;
};

const Slider = React.forwardRef<
	React.ElementRef<typeof SliderPrimitive.Root>,
	SliderProps
>(
	(
		{ className, value, defaultValue, min = 0, max = 100, label, ...props },
		ref,
	) => {
		const values = value ?? defaultValue ?? [min];
		return (
			<SliderPrimitive.Root
				ref={ref}
				value={value}
				defaultValue={defaultValue}
				min={min}
				max={max}
				className={cn(
					"relative flex w-full touch-none select-none items-center data-[disabled]:opacity-50",
					className,
				)}
				{...props}
			>
				<SliderPrimitive.Track
					className={cn(
						"relative h-1 w-full grow",
						"overflow-hidden rounded-full bg-muted",
					)}
				>
					<SliderPrimitive.Range className="absolute h-full bg-primary" />
				</SliderPrimitive.Track>
				{values.map((_, index) => (
					<SliderPrimitive.Thumb
						key={index === 0 ? "minimum" : "maximum"}
						aria-label={
							values.length === 1
								? label
								: `${label} ${index === 0 ? "minimum" : "maximum"}`
						}
						className={cn(
							"block size-3 shrink-0 rounded-full border border-ring bg-background",
							"ring-ring/50 transition-shadow focus-visible:outline-none focus-visible:ring-4",
							"disabled:pointer-events-none",
						)}
					/>
				))}
			</SliderPrimitive.Root>
		);
	},
);
Slider.displayName = "Slider";
export { Slider };
