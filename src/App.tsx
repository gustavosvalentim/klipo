import { useEffect, useMemo, useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type ClipboardItem = {
  text: string;
};

type Clipboard = ClipboardItem[];

type MenuItemProps = {
  label: string;
  onClick: () => void;
  active?: boolean;
};

const MenuItem = ({ label, onClick, active, ...props }: MenuItemProps) => (
  <button className={`menu__item ${active ? "active" : ""}`} onClick={onClick} {...props}>
    <span className="menu__title menu__text">{label}</span>
  </button>
);

const MenuSeparator = () => <div className="menu__separator" />;

const MenuItemGroup = ({ items, activeItem }: { items: MenuItemProps[], activeItem?: number }) => (
  <>
    <MenuSeparator />
    {items.map((item, idx) => (
      <MenuItem {...item} active={idx === activeItem} key={idx} />
    ))}
  </>
);

function App() {
  const [clipboard, setClipboard] = useState<Clipboard>([]);
  const [selectedItem, setSelectedItem] = useState<number | null>(null);

  const quitClipbox = () => invoke("quit_clipbox");

  const hideClipbox = () => invoke("hide_clipbox");

  const fetchClipboardHistory = async () => {
    try {
      let clipboard = await invoke<ClipboardItem[]>("list_clipboard_items");
      setClipboard(clipboard);
    } catch (error) {
      console.error("Failed to get clipboard history", error);
    }
  };

  const clearHistory = async () => {
    try {
      await invoke("clear_clipboard_items");
      fetchClipboardHistory();
    } catch (error) {
      console.error("Failed to clear clipboard history", error);
    }
  };

  const pasteFromSelection = async (text: string) => {
    try {
      await invoke("paste_from_selection", { text });
      fetchClipboardHistory();
    } catch (error) {
      console.error("Failed to paste from selection", error);
    }
  };

  const actionsMenuItems = [
    [
      { "label": "Clear History", "onClick": clearHistory },
      { "label": "Preferences...", "onClick": () => { } },
    ],
    [
      { "label": "Quit", "onClick": quitClipbox },
    ],
  ];

  const clipboardMenuItems = useMemo(() => clipboard.map((item) => ({
    "label": `${item.text}`,
    "onClick": () => pasteFromSelection(item.text),
  })), [clipboard]);

  const handleKeyDown = useCallback((event: KeyboardEvent) => {
    let newSelectedItem = selectedItem;
    switch (event.key) {
      case "ArrowUp":
        event.preventDefault();
        newSelectedItem = selectedItem !== null ? selectedItem - 1 : clipboard.length;
        break;
      case "ArrowDown":
        event.preventDefault();
        newSelectedItem = selectedItem !== null ? selectedItem + 1 : 0;
        break;
      case "Enter":
        event.preventDefault();
        if (selectedItem !== null && selectedItem >= 0 && selectedItem < clipboard.length) {
          pasteFromSelection(clipboard[selectedItem].text);
        }
        break;
      case "Escape":
        event.preventDefault();
        hideClipbox();
        break;
      default:
        break;
    }

    if (newSelectedItem !== null && (newSelectedItem < 0 || newSelectedItem >= clipboard.length)) {
      newSelectedItem = 0;
    }

    setSelectedItem(newSelectedItem);
  }, [clipboard, selectedItem]);

  const handleBlur = useCallback(() => {
    setSelectedItem(null)
  }, []);

  const handleFocus = useCallback(() => {
    fetchClipboardHistory();
  }, []);

  useEffect(() => {
    const unlisten = listen<string>("clipboard-changed", fetchClipboardHistory);

    return () => {
      unlisten.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("focus", handleFocus);
    window.addEventListener("blur", handleBlur);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("focus", handleFocus);
      window.removeEventListener("blur", handleBlur);
    };
  }, [handleKeyDown, handleFocus, handleBlur]);

  return (
    <div className="menu">
      <div className="menu__content">
        <div className="menu__label">
          <span className="menu__text">History</span>
        </div>

        <MenuSeparator />

        <div className="menu__history">
          {clipboardMenuItems.map((item, idx) => (
            <MenuItem {...item} active={idx === selectedItem} key={idx} />
          ))}
        </div>
      </div>

      <div className="menu__footer">
        {actionsMenuItems.map((items, idx) => (
          <MenuItemGroup items={items} key={idx} />
        ))}
      </div>
    </div>
  );
}

export default App;
