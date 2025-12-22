import type { ReactNode } from "react";

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
		<dialog className="modal" open={open}>
			<div className="modal-box">
				<h3 className="text-lg font-bold">{title}</h3>
				{description ? <p className="py-4">{description}</p> : null}
				{footer ?? (
					<div className="modal-action">
						<button type="button" className="btn" onClick={onCancel}>
							{cancelLabel}
						</button>
						<button
							type="button"
							className="btn btn-primary"
							onClick={onConfirm}
						>
							{confirmLabel}
						</button>
					</div>
				)}
			</div>
			<form method="dialog" className="modal-backdrop">
				<button type="button" onClick={onCancel}>
					close
				</button>
			</form>
		</dialog>
	);
}
