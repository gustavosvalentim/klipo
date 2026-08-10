import { Trash2 } from "react-feather";

type ClearHistoryButtonProps = {
	onClick: () => void;
};

export function ClearHistoryButton({ onClick }: ClearHistoryButtonProps) {
	return (
		<div className="group relative flex">
			<button
				type="button"
				aria-label="Clear history"
				className="cursor-pointer rounded-md p-1 hover:bg-[#0a84ff] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#0a84ff] focus-visible:ring-offset-2 focus-visible:ring-offset-transparent"
				onClick={onClick}
			>
				<Trash2 aria-hidden="true" focusable="false" className="h-4 w-4" />
			</button>
			<span
				role="tooltip"
				className="pointer-events-none absolute right-0 top-full z-10 mt-2 whitespace-nowrap rounded-md bg-black/80 px-2 py-1 text-xs text-white opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
			>
				Clear history
			</span>
		</div>
	);
}
