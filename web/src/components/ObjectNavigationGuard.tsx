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
	save: () => Promise<boolean>;
	discard: () => void;
};

type ObjectNavigationGuardValue = {
	registerDirtySections: (
		ownerId: string,
		getSections: () => ObjectNavigationDirtySection[],
	) => () => void;
	requestNavigation: (navigate: () => void) => void;
};

type RegisteredSections = {
	getSections: () => ObjectNavigationDirtySection[];
};

type PendingNavigation = {
	navigate: () => void;
	sections: ObjectNavigationDirtySection[];
	index: number;
};

const ObjectNavigationGuardContext =
	createContext<ObjectNavigationGuardValue | null>(null);

const fallbackGuard: ObjectNavigationGuardValue = {
	registerDirtySections: () => () => undefined,
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

	const requestNavigation = useCallback((navigate: () => void) => {
		const dirtySections = Array.from(registeredSectionsRef.current.values())
			.flatMap((registration) => registration.getSections())
			.filter((section) => section.isDirty());
		if (dirtySections.length === 0) {
			navigate();
			return;
		}
		setPendingNavigation({ navigate, sections: dirtySections, index: 0 });
	}, []);

	const value = useMemo<ObjectNavigationGuardValue>(
		() => ({ registerDirtySections, requestNavigation }),
		[registerDirtySections, requestNavigation],
	);
	const currentSection =
		pendingNavigation?.sections[pendingNavigation.index] ?? null;

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
			if (await currentSection.save()) {
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

	return (
		<ObjectNavigationGuardContext.Provider value={value}>
			{children}
			<ConfirmDialog
				open={currentSection !== null}
				title={`Unsaved ${currentSection?.label ?? ""} changes`}
				description="Save or discard this section before opening another object."
				onCancel={() => setPendingNavigation(null)}
				footer={
					<div className="flex flex-wrap justify-end gap-2">
						<Button
							type="button"
							variant="ghost"
							disabled={isSaving}
							onClick={() => setPendingNavigation(null)}
						>
							Keep editing
						</Button>
						<Button
							type="button"
							variant="secondary"
							disabled={isSaving}
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

export function useObjectNavigationDirtySections(
	ownerId: string,
	sections: ObjectNavigationDirtySection[],
) {
	const { registerDirtySections } = useObjectNavigationGuard();
	const sectionsRef = useRef(sections);
	sectionsRef.current = sections;

	useEffect(
		() => registerDirtySections(ownerId, () => sectionsRef.current),
		[ownerId, registerDirtySections],
	);
}
