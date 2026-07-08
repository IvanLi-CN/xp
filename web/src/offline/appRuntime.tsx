import type { ReactNode } from "react";
import { createContext, useContext, useEffect, useMemo, useState } from "react";

type AppRuntimeContextValue = {
	isOnline: boolean;
	isReadOnly: boolean;
	readOnlyReason: string | null;
};

const DEFAULT_APP_RUNTIME: AppRuntimeContextValue = {
	isOnline: true,
	isReadOnly: false,
	readOnlyReason: null,
};

const AppRuntimeContext =
	createContext<AppRuntimeContextValue>(DEFAULT_APP_RUNTIME);

export function AppRuntimeProvider(props: {
	children: ReactNode;
	initialIsOnline?: boolean;
}) {
	const [isOnline, setIsOnline] = useState(() => {
		if (typeof props.initialIsOnline === "boolean") {
			return props.initialIsOnline;
		}
		if (typeof navigator === "undefined") {
			return true;
		}
		return navigator.onLine;
	});

	useEffect(() => {
		const handleOnline = () => setIsOnline(true);
		const handleOffline = () => setIsOnline(false);
		window.addEventListener("online", handleOnline);
		window.addEventListener("offline", handleOffline);
		return () => {
			window.removeEventListener("online", handleOnline);
			window.removeEventListener("offline", handleOffline);
		};
	}, []);

	const value = useMemo<AppRuntimeContextValue>(
		() => ({
			isOnline,
			isReadOnly: !isOnline,
			readOnlyReason: !isOnline
				? "Offline read-only mode is active. Cached data remains available, " +
					"but writes are blocked until the connection returns."
				: null,
		}),
		[isOnline],
	);

	return (
		<AppRuntimeContext.Provider value={value}>
			{props.children}
		</AppRuntimeContext.Provider>
	);
}

export function useAppRuntime() {
	return useContext(AppRuntimeContext);
}
