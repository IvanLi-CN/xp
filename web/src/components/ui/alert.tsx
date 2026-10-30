import * as React from "react";

import { cn } from "@/lib/utils";

export type AlertTone = "info" | "warning" | "success" | "error";

type AlertProps = React.HTMLAttributes<HTMLElement> & {
	tone: AlertTone;
	as?: "div" | "p" | "output" | "section";
};

/** A non-live callout by default; callers opt into status or alert announcements. */
export function Alert({ as = "div", tone, className, ...props }: AlertProps) {
	return React.createElement(as, {
		...props,
		className: cn("xp-alert", `xp-alert-${tone}`, className),
	});
}
