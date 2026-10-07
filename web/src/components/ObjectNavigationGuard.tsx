import { useBlocker } from "@tanstack/react-router";
import type { ReactNode } from "react";
import {
	createContext,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";

import { Button } from "./Button";
import { ConfirmDialog } from "./ConfirmDialog";

export type ObjectNavigationDirtySection = {
	id: string;
	label: string;
	isDirty: () => boolean;
	isBusy?: () => boolean;
	save: () => Promise<boolean>;
	discard: () => void;
};

type ObjectNavigationGuardValue = {
	registerDirtySections: (
		ownerId: string,
		getSections: () => ObjectNavigationDirtySection[],
	) => () => void;
	refresh: () => void;
	getDirtySections: () => ObjectNavigationDirtySection[];
	requestNavigation: (navigate: () => void, onCancel?: () => void) => void;
};

type RegisteredSections = {
	getSections: () => ObjectNavigationDirtySection[];
};

type PendingNavigation = {
	navigate: () => void;
	onCancel?: () => void;
	sections: ObjectNavigationDirtySection[];
	index: number;
};

const ObjectNavigationGuardContext =
	createContext<ObjectNavigationGuardValue | null>(null);

const fallbackGuard: ObjectNavigationGuardValue = {
	registerDirtySections: () => () => undefined,
	refresh: () => undefined,
	getDirtySections: () => [],
	requestNavigation: (navigate) => navigate(),
};

export function ObjectNavigationGuardProvider({
	children,
}: {
	children: ReactNode;
}) {
	const registeredSectionsRef = useRef(new Map<string, RegisteredSections>());
	const [pendingNavigation, setPendingNavigation] =
		useState<PendingNavigation | null>(null);
	const [isSaving, setIsSaving] = useState(false);
	const [, refresh] = useState(0);

	const registerDirtySections = useCallback(
		(ownerId: string, getSections: () => ObjectNavigationDirtySection[]) => {
			const registration: RegisteredSections = { getSections };
			registeredSectionsRef.current.set(ownerId, registration);
			return () => {
				if (registeredSectionsRef.current.get(ownerId) === registration) {
					registeredSectionsRef.current.delete(ownerId);
				}
			};
		},
		[],
	);

	const getDirtySections = useCallback(
		() =>
			Array.from(registeredSectionsRef.current.values())
				.flatMap((registration) => registration.getSections())
				.filter((section) => section.isDirty()),
		[],
	);

	const requestNavigation = useCallback(
		(navigate: () => void, onCancel?: () => void) => {
			const dirtySections = getDirtySections();
			if (dirtySections.length === 0) {
				navigate();
				return;
			}
			setPendingNavigation({
				navigate,
				onCancel,
				sections: dirtySections,
				index: 0,
			});
		},
		[getDirtySections],
	);

	const value = useMemo<ObjectNavigationGuardValue>(
		() => ({
			getDirtySections,
			registerDirtySections,
			refresh: () => refresh((current) => current + 1),
			requestNavigation,
		}),
		[getDirtySections, registerDirtySections, requestNavigation],
	);
	const currentSection =
		pendingNavigation?.sections[pendingNavigation.index] ?? null;
	const currentSectionBusy = currentSection?.isBusy?.() ?? false;

	function continueNavigation() {
		if (!pendingNavigation) return;
		const nextIndex = pendingNavigation.index + 1;
		if (nextIndex < pendingNavigation.sections.length) {
			setPendingNavigation({ ...pendingNavigation, index: nextIndex });
			return;
		}
		const { navigate } = pendingNavigation;
		setPendingNavigation(null);
		navigate();
	}

	async function saveAndContinue() {
		if (!currentSection || isSaving) return;
		setIsSaving(true);
		try {
			if ((await currentSection.save()) || !currentSection.isDirty()) {
				continueNavigation();
			}
		} finally {
			setIsSaving(false);
		}
	}

	function discardAndContinue() {
		if (!currentSection || isSaving) return;
		currentSection.discard();
		continueNavigation();
	}

	function cancelNavigation() {
		pendingNavigation?.onCancel?.();
		setPendingNavigation(null);
	}

	return (
		<ObjectNavigationGuardContext.Provider value={value}>
			{children}
			<ConfirmDialog
				open={currentSection !== null}
				title={`Unsaved ${currentSection?.label ?? ""} changes`}
				description="Save or discard this section before opening another object."
				onCancel={cancelNavigation}
				footer={
					<div className="flex flex-wrap justify-end gap-2">
						<Button
							type="button"
							variant="ghost"
							disabled={isSaving || currentSectionBusy}
							onClick={cancelNavigation}
						>
							Keep editing
						</Button>
						<Button
							type="button"
							variant="secondary"
							disabled={isSaving || currentSectionBusy}
							onClick={discardAndContinue}
						>
							Discard and continue
						</Button>
						<Button
							type="button"
							loading={isSaving}
							onClick={() => void saveAndContinue()}
						>
							Save and continue
						</Button>
					</div>
				}
			/>
		</ObjectNavigationGuardContext.Provider>
	);
}

export function useObjectNavigationGuard() {
	return useContext(ObjectNavigationGuardContext) ?? fallbackGuard;
}

export function useObjectNavigationBrowserBlocker() {
	const { getDirtySections, requestNavigation } = useObjectNavigationGuard();
	const blocker = useBlocker({
		shouldBlockFn: () => getDirtySections().length > 0,
		enableBeforeUnload: () => getDirtySections().length > 0,
		withResolver: true,
	});

	useEffect(() => {
		if (blocker.status !== "blocked") return;
		requestNavigation(
			() => blocker.proceed?.(),
			() => blocker.reset?.(),
		);
	}, [blocker, requestNavigation]);
}

export function useObjectNavigationDirtySections(
	ownerId: string,
	sections: ObjectNavigationDirtySection[],
) {
	const { refresh, registerDirtySections } = useObjectNavigationGuard();
	const sectionsRef = useRef(sections);
	sectionsRef.current = sections;
	const stableSectionsRef = useRef(
		new Map<string, ObjectNavigationDirtySection>(),
	);
	const getSections = useCallback(() => {
		return sectionsRef.current.map((section) => {
			let stableSection = stableSectionsRef.current.get(section.id);
			if (!stableSection) {
				const sectionId = section.id;
				stableSection = {
					id: sectionId,
					label: section.label,
					isDirty: () =>
						sectionsRef.current
							.find((item) => item.id === sectionId)
							?.isDirty() ?? false,
					isBusy: () =>
						sectionsRef.current
							.find((item) => item.id === sectionId)
							?.isBusy?.() ?? false,
					save: () =>
						sectionsRef.current.find((item) => item.id === sectionId)?.save() ??
						Promise.resolve(false),
					discard: () => {
						sectionsRef.current
							.find((item) => item.id === sectionId)
							?.discard();
					},
				};
				stableSectionsRef.current.set(sectionId, stableSection);
			}
			stableSection.label = section.label;
			return stableSection;
		});
	}, []);
	const sectionState = sections
		.map(
			(section) =>
				`${section.isDirty() ? "dirty" : "clean"}:${section.isBusy?.() ? "busy" : "idle"}`,
		)
		.join("|");

	useEffect(
		() => registerDirtySections(ownerId, getSections),
		[getSections, ownerId, registerDirtySections],
	);
	const refreshSectionState = useCallback(() => {
		void sectionState;
		refresh();
	}, [refresh, sectionState]);
	useEffect(() => refreshSectionState(), [refreshSectionState]);
}
