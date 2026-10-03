import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type { DesktopCapabilities } from "./capabilities";
import { logError } from "./log";
import { presentationForSession, shortcutLabel } from "./platform";
import {
	type ShortcutField,
	type ShortcutSettings,
	shortcutFields,
	shortcutFromEvent,
} from "./shortcuts";

export function SettingsView() {
	const [saved, setSaved] = useState<ShortcutSettings | null>(null);
	const [draft, setDraft] = useState<ShortcutSettings | null>(null);
	const [recording, setRecording] = useState<ShortcutField | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [session, setSession] = useState<DesktopCapabilities["session"]>();
	const platformPresentation = presentationForSession(session);
	const draftRef = useRef(draft);
	const savedRef = useRef(saved);
	const saving = useRef(false);
	const [isSaving, setIsSaving] = useState(false);
	draftRef.current = draft;
	savedRef.current = saved;

	const load = useCallback(async () => {
		if (
			saving.current ||
			JSON.stringify(draftRef.current) !== JSON.stringify(savedRef.current)
		)
			return;
		try {
			const settings = await invoke<ShortcutSettings>("get_shortcuts");
			if (
				saving.current ||
				JSON.stringify(draftRef.current) !== JSON.stringify(savedRef.current)
			)
				return;
			setSaved(settings);
			setDraft(settings);
			setError(null);
		} catch (reason) {
			logError("Failed to load keyboard shortcuts", reason);
			setError(String(reason));
		}
	}, []);

	useEffect(() => {
		load();
		window.addEventListener("focus", load);
		return () => window.removeEventListener("focus", load);
	}, [load]);

	useEffect(() => {
		void invoke<DesktopCapabilities>("get_capabilities")
			.then((capabilities) => setSession(capabilities.session))
			.catch((reason) =>
				logError("Failed to get desktop capabilities", reason),
			);
	}, []);

	useEffect(() => {
		const closeSettings = () => {
			void invoke("close_settings").catch((reason) => {
				logError("Failed to close settings window", reason);
				setError(String(reason));
			});
		};
		const record = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				event.preventDefault();
				event.stopPropagation();
				setRecording(null);
				closeSettings();
				return;
			}
			if (saving.current || !recording) return;
			event.preventDefault();
			event.stopPropagation();
			const shortcut = shortcutFromEvent(event);
			if (!shortcut) {
				setError("Escape and modifier-only shortcuts cannot be used.");
				return;
			}
			setDraft((current) => {
				const updated = current && { ...current, [recording]: shortcut };
				draftRef.current = updated;
				return updated;
			});
			setError(null);
			setRecording(null);
		};
		window.addEventListener("keydown", record, true);
		return () => window.removeEventListener("keydown", record, true);
	}, [recording]);

	if (!draft || !saved) {
		if (error)
			return (
				<main
					className={`settings settings__error ${platformPresentation.className}`}
					role="alert"
				>
					{error}
				</main>
			);
		return (
			<main className={`settings ${platformPresentation.className}`}>
				Loading settings…
			</main>
		);
	}

	const save = async () => {
		if (saving.current) return;
		saving.current = true;
		setIsSaving(true);
		try {
			const updated = await invoke<ShortcutSettings>("save_shortcuts", {
				settings: draft,
			});
			setSaved(updated);
			setDraft(updated);
			setError(null);
		} catch (reason) {
			logError("Failed to save keyboard shortcuts", reason);
			setError(String(reason));
		} finally {
			saving.current = false;
			setIsSaving(false);
		}
	};

	return (
		<main className={`settings ${platformPresentation.className}`}>
			<h1>Keyboard shortcuts</h1>
			<p>
				Click a shortcut, then press one key combination. Escape always closes
				Klipo.
			</p>
			{shortcutFields.map(([field, label]) => (
				<label className="settings__field" key={field}>
					<span>{label}</span>
					<button
						type="button"
						disabled={isSaving}
						className={
							recording === field
								? "settings__shortcut is-recording"
								: "settings__shortcut"
						}
						onClick={() => {
							setRecording(field);
							setError(null);
						}}
					>
						{recording === field
							? "Press shortcut…"
							: shortcutLabel(draft[field])}
					</button>
				</label>
			))}
			{error && (
				<p className="settings__error" role="alert">
					{error}
				</p>
			)}
			<div className="settings__actions">
				<button
					type="button"
					onClick={save}
					disabled={isSaving || JSON.stringify(saved) === JSON.stringify(draft)}
				>
					Save changes
				</button>
			</div>
		</main>
	);
}
