import type { ReactNode } from "react";
import { createContext, useContext, useMemo } from "react";
import { Toaster, toast } from "sonner";

import { cn } from "@/lib/utils";

import { useUiPrefsOptional } from "./UiPrefs";

export type ToastVariant = "success" | "error" | "info";

type ToastContextValue = {
	pushToast: (toast: { variant: ToastVariant; message: string }) => void;
};

const ToastContext = createContext<ToastContextValue | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
	const prefs = useUiPrefsOptional();
	const value = useMemo<ToastContextValue>(
		() => ({
			pushToast: ({ variant, message }) => {
				const base = {
					duration: 4000,
					className: cn(
						"rounded-xl border border-border bg-popover text-popover-foreground shadow-lg",
					),
				};
				if (variant === "success") {
					toast.success(message, base);
					return;
				}
				if (variant === "error") {
					toast.error(message, base);
					return;
				}
				toast(message, base);
			},
		}),
		[],
	);

	return (
		<ToastContext.Provider value={value}>
			{children}
			<Toaster
				closeButton
				position="bottom-right"
				richColors
				theme={prefs?.resolvedTheme ?? "light"}
			/>
		</ToastContext.Provider>
	);
}

export function useToast(): ToastContextValue {
	const ctx = useContext(ToastContext);
	if (!ctx) {
		throw new Error("useToast must be used within <ToastProvider />");
	}
	return ctx;
}
