export type ListItemProps = {
	label: string;
	onClick?: () => void;
	active?: boolean;
};

type ListItemButtonProps = React.PropsWithChildren & {
	className?: string;
	onClick?: () => void;
	active?: boolean;
};

const ListItemButtonStyle =
	"flex-1 min-w-0 h-[24px] border-0 rounded-sm text-left overflow-hidden";

const ActiveListItemButtonStyle = "bg-[#0a84ff]";

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
	...props
}: ListItemProps) => (
	<div className="flex w-full items-center my-1">
		<ListItemButton
			onClick={onClick}
			className="hover:bg-[#0a84ff] px-2"
			active={active}
			{...props}
		>
			<span className="block text-sm min-w-0 text-nowrap whitespace-nowrap text-ellipsis overflow-hidden">
				{label}
			</span>
		</ListItemButton>
	</div>
);
