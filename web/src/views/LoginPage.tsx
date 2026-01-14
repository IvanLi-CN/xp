import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";

import { verifyAdminToken } from "../api/adminAuth";
import { isBackendApiError } from "../api/backendError";
import { Button } from "../components/Button";
import { useUiPrefs } from "../components/UiPrefs";
import {
	ADMIN_TOKEN_STORAGE_KEY,
	clearAdminToken,
	readAdminToken,
	writeAdminToken,
} from "../components/auth";
import { parseAdminTokenInput } from "../utils/adminToken";

function formatError(err: unknown): string {
	if (isBackendApiError(err)) {
		const code = err.code ? ` ${err.code}` : "";
		return `${err.status}${code}: ${err.message}`;
	}
	if (err instanceof Error) return err.message;
	return String(err);
}

export function LoginPage() {
	const navigate = useNavigate();
	const prefs = useUiPrefs();
	const [token, setToken] = useState(() => readAdminToken());
	const [draft, setDraft] = useState(() => readAdminToken());
	const [isVerifying, setIsVerifying] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const inputClass =
		prefs.density === "compact"
			? "input input-bordered input-sm font-mono w-full"
			: "input input-bordered font-mono w-full";

	return (
		<div className="min-h-screen bg-base-200 flex items-center justify-center px-6">
			<div className="card bg-base-100 shadow w-full max-w-lg">
				<div className="card-body space-y-4">
					<div>
						<h1 className="text-2xl font-bold">Admin login</h1>
						<p className="text-sm opacity-70">
							Enter the admin token to access the control plane UI.
						</p>
					</div>
					<div className="space-y-2">
						<p className="text-xs uppercase tracking-wide opacity-50">
							Stored in localStorage key
						</p>
						<div className="rounded-box border border-base-200 bg-base-200/60 px-4 py-2 w-full">
							<p className="font-mono text-sm">{ADMIN_TOKEN_STORAGE_KEY}</p>
						</div>
					</div>
					<label className="form-control">
						<div className="label">
							<span className="label-text">Token</span>
						</div>
						<input
							type="password"
							className={inputClass}
							placeholder="e.g. admin-token"
							value={draft}
							onChange={(event) => {
								setDraft(event.target.value);
								setError(null);
							}}
						/>
					</label>
					{token.length === 0 ? (
						<p className="text-warning">
							No token set. Please add a token to continue.
						</p>
					) : (
						<p className="text-sm opacity-70">
							Token stored (length {token.length}).
						</p>
					)}
					{error ? <p className="text-sm text-error">{error}</p> : null}
					<div className="card-actions justify-end gap-2">
						<Button
							variant="ghost"
							onClick={() => {
								clearAdminToken();
								setToken("");
								setDraft("");
								setError(null);
							}}
						>
							Clear
						</Button>
						<Button
							variant="secondary"
							loading={isVerifying}
							disabled={isVerifying}
							onClick={async () => {
								const parsed = parseAdminTokenInput(draft);
								if ("error" in parsed) {
									setError(parsed.error);
									return;
								}
								setIsVerifying(true);
								setError(null);
								try {
									await verifyAdminToken(parsed.token);
									writeAdminToken(parsed.token);
									setToken(parsed.token);
									setDraft(parsed.token);
									navigate({ to: "/" });
								} catch (err) {
									setError(formatError(err));
								} finally {
									setIsVerifying(false);
								}
							}}
						>
							Save & Continue
						</Button>
					</div>
				</div>
			</div>
		</div>
	);
}
