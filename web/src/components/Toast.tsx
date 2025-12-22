import type { ReactNode } from "react";
import {
	createContext,
	useCallback,
	useContext,
	useMemo,
	useState,
} from "react";

export type ToastVariant = "success" | "error" | "info";

type ToastItem = {
	id: string;
	variant: ToastVariant;
	message: string;
};

type ToastContextValue = {
	pushToast: (toast: Omit<ToastItem, "id">) => void;
};

const ToastContext = createContext<ToastContextValue | null>(null);

function newToastId(): string {
	if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
		return crypto.randomUUID();
	}
	return `toast_${Date.now()}_${Math.random().toString(16).slice(2)}`;
}

export function ToastProvider({ children }: { children: ReactNode }) {
	const [toasts, setToasts] = useState<ToastItem[]>([]);

	const dismissToast = useCallback((id: string) => {
		setToasts((prev) => prev.filter((toast) => toast.id !== id));
	}, []);

	const pushToast = useCallback(
		(toast: Omit<ToastItem, "id">) => {
			const id = newToastId();
			setToasts((prev) => [...prev, { ...toast, id }]);
			setTimeout(() => dismissToast(id), 4000);
		},
		[dismissToast],
	);

	const value = useMemo(() => ({ pushToast }), [pushToast]);

	return (
		<ToastContext.Provider value={value}>
			{children}
			<ToastViewport toasts={toasts} onDismiss={dismissToast} />
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

export function ToastViewport({
	toasts,
	onDismiss,
}: {
	toasts: ToastItem[];
	onDismiss: (id: string) => void;
}) {
	return (
		<div className="toast toast-end toast-bottom">
			{toasts.map((toast) => (
				<div
					key={toast.id}
					className={[
						"alert",
						toast.variant === "success"
							? "alert-success"
							: toast.variant === "error"
								? "alert-error"
								: "alert-info",
					].join(" ")}
				>
					<span>{toast.message}</span>
					<button
						type="button"
						className="btn btn-ghost btn-xs"
						onClick={() => onDismiss(toast.id)}
					>
						Close
					</button>
				</div>
			))}
		</div>
	);
}
