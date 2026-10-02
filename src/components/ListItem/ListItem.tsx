export type ListItemProps = {
	label: string;
	onClick?: () => void;
	active?: boolean;
	preview?: string;
};

export const ListItem = ({
	label,
	onClick,
	active,
	preview,
	...props
}: ListItemProps) => (
	<div className="flex w-full items-center my-1">
		<button
			type="button"
			onClick={onClick}
			className={`list-item__button flex-1 min-w-0 h-[24px] border-0 rounded-sm text-left overflow-hidden px-2${active ? " is-active" : ""}`}
			{...props}
		>
			{preview ? (
				<img
					src={preview}
					alt={label}
					className="inline-block w-[20px] h-[20px] object-contain align-middle"
				/>
			) : (
				<span className="block text-sm min-w-0 text-nowrap whitespace-nowrap text-ellipsis overflow-hidden">
					{label}
				</span>
			)}
		</button>
	</div>
);
