import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";

import { Button } from "../components/Button";
import {
	ADMIN_TOKEN_STORAGE_KEY,
	clearAdminToken,
	readAdminToken,
	writeAdminToken,
} from "../components/auth";

export function LoginPage() {
	const navigate = useNavigate();
	const [token, setToken] = useState(() => readAdminToken());
	const [draft, setDraft] = useState(() => readAdminToken());

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
						<p className="font-mono text-sm">{ADMIN_TOKEN_STORAGE_KEY}</p>
					</div>
					<label className="form-control">
						<div className="label">
							<span className="label-text">Token</span>
						</div>
						<input
							type="password"
							className="input input-bordered font-mono"
							placeholder="e.g. admin-token"
							value={draft}
							onChange={(event) => setDraft(event.target.value)}
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
					<div className="card-actions justify-end gap-2">
						<Button
							variant="ghost"
							onClick={() => {
								clearAdminToken();
								setToken("");
								setDraft("");
							}}
						>
							Clear
						</Button>
						<Button
							variant="secondary"
							onClick={() => {
								const next = draft.trim();
								writeAdminToken(next);
								setToken(next);
								setDraft(next);
								if (next.length > 0) {
									navigate({ to: "/" });
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
