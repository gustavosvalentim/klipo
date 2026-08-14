export type ListItemProps = {
	label: string;
	onClick?: () => void;
	active?: boolean;
	preview?: string;
};

type ListItemButtonProps = React.PropsWithChildren & {
	className?: string;
	onClick?: () => void;
	active?: boolean;
};

const ListItemButtonStyle =
	"list-item__button flex-1 min-w-0 h-[24px] border-0 rounded-sm text-left overflow-hidden";

const ActiveListItemButtonStyle = "is-active";

const ListItemButton = ({
	onClick,
	active,
	className,
	...props
}: ListItemButtonProps) => {
	const buttonStyle = [ListItemButtonStyle, className];

	if (active) {
		buttonStyle.push(ActiveListItemButtonStyle);
	}

	return (
		<button className={buttonStyle.join(" ")} onClick={onClick} {...props}>
			{props.children}
		</button>
	);
};

export const ListItem = ({
	label,
	onClick,
	active,
	preview,
	...props
}: ListItemProps) => (
	<div className="flex w-full items-center my-1">
		<ListItemButton
			onClick={onClick}
			className="px-2"
			active={active}
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
		</ListItemButton>
	</div>
);
