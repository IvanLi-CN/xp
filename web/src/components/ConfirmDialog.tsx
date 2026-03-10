import type { ReactNode } from "react";

import {
	AlertDialog,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
} from "@/components/ui/alert-dialog";

import { Button } from "./Button";

type ConfirmDialogProps = {
	open: boolean;
	title: string;
	description?: string;
	confirmLabel?: string;
	cancelLabel?: string;
	onConfirm?: () => void;
	onCancel?: () => void;
	footer?: ReactNode;
};

export function ConfirmDialog({
	open,
	title,
	description,
	confirmLabel = "Confirm",
	cancelLabel = "Cancel",
	onConfirm,
	onCancel,
	footer,
}: ConfirmDialogProps) {
	return (
		<AlertDialog open={open} onOpenChange={(next) => !next && onCancel?.()}>
			<AlertDialogContent>
				<AlertDialogHeader>
					<AlertDialogTitle>{title}</AlertDialogTitle>
					{description ? (
						<AlertDialogDescription>{description}</AlertDialogDescription>
					) : (
						<AlertDialogDescription className="sr-only">
							Confirm this action.
						</AlertDialogDescription>
					)}
				</AlertDialogHeader>
				{footer ?? (
					<AlertDialogFooter>
						<AlertDialogCancel asChild>
							<Button type="button" variant="ghost">
								{cancelLabel}
							</Button>
						</AlertDialogCancel>
						<Button type="button" onClick={onConfirm}>
							{confirmLabel}
						</Button>
					</AlertDialogFooter>
				)}
			</AlertDialogContent>
		</AlertDialog>
	);
}
