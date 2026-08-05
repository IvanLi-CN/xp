import type { ErrorInfo, ReactNode } from "react";
import React from "react";

import { renderDocumentFallback } from "../runtime/documentFallback";
import {
	type FrameworkErrorCategory,
	classifyFrameworkError,
} from "../runtime/frameworkErrorRecovery";
import { FrameworkErrorRecovery } from "./FrameworkErrorRecovery";

type FrameworkErrorBoundaryState = {
	error: unknown | null;
	category?: FrameworkErrorCategory;
	repeatFailure: boolean;
};

const FAILURE_STORAGE_KEY = "xp_framework_error_recovery";

function readFailureCounts(): Record<string, number> {
	try {
		const raw = sessionStorage.getItem(FAILURE_STORAGE_KEY);
		if (!raw) return {};
		const parsed = JSON.parse(raw) as Record<string, unknown>;
		const counts: Record<string, number> = {};
		for (const [key, value] of Object.entries(parsed)) {
			if (typeof value === "number" && value > 0) counts[key] = value;
		}
		return counts;
	} catch {
		return {};
	}
}

function recordFailure(category: FrameworkErrorCategory): boolean {
	const counts = readFailureCounts();
	const repeated = (counts[category] ?? 0) > 0;
	counts[category] = Math.min((counts[category] ?? 0) + 1, 3);
	try {
		sessionStorage.setItem(FAILURE_STORAGE_KEY, JSON.stringify(counts));
	} catch {
		// Session storage is optional; recovery must still render without it.
	}
	return repeated;
}

export class FrameworkErrorBoundary extends React.Component<
	{ children: ReactNode },
	FrameworkErrorBoundaryState
> {
	state: FrameworkErrorBoundaryState = {
		error: null,
		repeatFailure: false,
	};

	static getDerivedStateFromError(
		error: unknown,
	): Partial<FrameworkErrorBoundaryState> {
		return { error };
	}

	componentDidCatch(error: unknown, _errorInfo: ErrorInfo) {
		const category = classifyFrameworkError(error);
		this.setState({
			category,
			repeatFailure: recordFailure(category),
		});
	}

	render() {
		if (this.state.error === null) return this.props.children;
		return (
			<FrameworkErrorRecovery
				error={this.state.error}
				category={this.state.category}
				repeatFailure={this.state.repeatFailure}
			/>
		);
	}
}

type DocumentFallbackBoundaryProps = {
	children: ReactNode;
};

type DocumentFallbackBoundaryState = {
	hasError: boolean;
};

export class DocumentFallbackBoundary extends React.Component<
	DocumentFallbackBoundaryProps,
	DocumentFallbackBoundaryState
> {
	state: DocumentFallbackBoundaryState = { hasError: false };

	static getDerivedStateFromError(): DocumentFallbackBoundaryState {
		return { hasError: true };
	}

	componentDidMount() {
		document
			.getElementById("root")
			?.setAttribute("data-xp-react-ready", "true");
	}

	componentDidCatch(error: unknown) {
		const showFallback = () => renderDocumentFallback(error);
		if (typeof queueMicrotask === "function") {
			queueMicrotask(showFallback);
		} else {
			window.setTimeout(showFallback, 0);
		}
	}

	render() {
		return this.state.hasError ? null : this.props.children;
	}
}

export function installDocumentFallbackHandlers(rootElement: HTMLElement) {
	if (typeof window === "undefined") return () => {};

	const showIfBootstrapFailed = (error: unknown) => {
		if (rootElement.dataset.xpReactReady === "true") return;
		renderDocumentFallback(error);
	};
	const handleError = (event: ErrorEvent) => {
		showIfBootstrapFailed(event.error ?? event.message);
	};
	const handleRejection = (event: PromiseRejectionEvent) => {
		showIfBootstrapFailed(event.reason);
	};

	window.addEventListener("error", handleError);
	window.addEventListener("unhandledrejection", handleRejection);
	return () => {
		window.removeEventListener("error", handleError);
		window.removeEventListener("unhandledrejection", handleRejection);
	};
}
