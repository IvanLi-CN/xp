import type { ReactNode } from "react";
import { createContext, useContext, useEffect, useMemo, useState } from "react";

export type UiThemePreference = "system" | "light" | "dark";
export type UiThemeResolved = "light" | "dark";
export type UiDensity = "comfortable" | "compact";

export const UI_THEME_STORAGE_KEY = "xp_ui_theme";
export const UI_DENSITY_STORAGE_KEY = "xp_ui_density";

type UiPrefsContextValue = {
	theme: UiThemePreference;
	resolvedTheme: UiThemeResolved;
	setTheme: (next: UiThemePreference) => void;
	density: UiDensity;
	setDensity: (next: UiDensity) => void;
};

const UiPrefsContext = createContext<UiPrefsContextValue | null>(null);

function safeLocalStorageGet(key: string): string | null {
	try {
		return localStorage.getItem(key);
	} catch {
		return null;
	}
}

function safeLocalStorageSet(key: string, value: string): void {
	try {
		localStorage.setItem(key, value);
	} catch {
		// ignore
	}
}

function parseThemePreference(value: string | null): UiThemePreference {
	return value === "light" || value === "dark" || value === "system"
		? value
		: "system";
}

function parseDensity(value: string | null): UiDensity {
	return value === "compact" || value === "comfortable" ? value : "comfortable";
}

function resolveTheme(preference: UiThemePreference): UiThemeResolved {
	if (preference === "light" || preference === "dark") return preference;
	const prefersDark =
		typeof window !== "undefined" &&
		typeof window.matchMedia === "function" &&
		window.matchMedia("(prefers-color-scheme: dark)").matches;
	return prefersDark ? "dark" : "light";
}

function applyResolvedTheme(resolved: UiThemeResolved): void {
	const themeName = resolved === "dark" ? "xp-dark" : "xp-light";
	document.documentElement.setAttribute("data-theme", themeName);
}

function applyDensity(density: UiDensity): void {
	document.documentElement.setAttribute("data-density", density);
}

export function UiPrefsProvider({ children }: { children: ReactNode }) {
	const [theme, setThemeState] = useState<UiThemePreference>(() =>
		parseThemePreference(safeLocalStorageGet(UI_THEME_STORAGE_KEY)),
	);
	const [density, setDensityState] = useState<UiDensity>(() =>
		parseDensity(safeLocalStorageGet(UI_DENSITY_STORAGE_KEY)),
	);
	const [resolvedTheme, setResolvedTheme] = useState<UiThemeResolved>(() =>
		resolveTheme(theme),
	);

	useEffect(() => {
		const nextResolved = resolveTheme(theme);
		setResolvedTheme(nextResolved);
		applyResolvedTheme(nextResolved);
		safeLocalStorageSet(UI_THEME_STORAGE_KEY, theme);
	}, [theme]);

	useEffect(() => {
		applyDensity(density);
		safeLocalStorageSet(UI_DENSITY_STORAGE_KEY, density);
	}, [density]);

	useEffect(() => {
		if (theme !== "system" || typeof window.matchMedia !== "function") return;
		const query = window.matchMedia("(prefers-color-scheme: dark)");
		const onChange = () => {
			const nextResolved = resolveTheme("system");
			setResolvedTheme(nextResolved);
			applyResolvedTheme(nextResolved);
		};
		query.addEventListener("change", onChange);
		return () => query.removeEventListener("change", onChange);
	}, [theme]);

	const value = useMemo<UiPrefsContextValue>(
		() => ({
			theme,
			resolvedTheme,
			setTheme: setThemeState,
			density,
			setDensity: setDensityState,
		}),
		[theme, resolvedTheme, density],
	);

	return (
		<UiPrefsContext.Provider value={value}>{children}</UiPrefsContext.Provider>
	);
}

export function useUiPrefs(): UiPrefsContextValue {
	const ctx = useUiPrefsOptional();
	if (!ctx)
		throw new Error("useUiPrefs must be used within <UiPrefsProvider />");
	return ctx;
}

export function useUiPrefsOptional(): UiPrefsContextValue | null {
	return useContext(UiPrefsContext);
}
