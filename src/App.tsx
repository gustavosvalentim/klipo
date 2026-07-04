import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./App.css";

type ClipboardItem = {
  text: string;
};

type Clipboard = ClipboardItem[];

function App() {
  const [clipboard, setClipboard] = useState<Clipboard>([]);

  const quitClipbox = () => invoke("quit_clipbox");

  const getClipboardHistory = () => {
    invoke<ClipboardItem[]>("list_clipboard_items").then((history: Clipboard) => {
      console.log(history);
      setClipboard(history);
    });
  };

  const clearHistory = async () => {
    try {
      await invoke("clear_clipboard_items");
      getClipboardHistory();
    } catch (error) {
      console.error("Failed to clear clipboard history", error);
    }
  };

  const pasteFromSelection = async (text: string) => {
    try {
      await invoke("paste_from_selection", { text });
      getClipboardHistory();
    } catch (error) {
      console.error("Failed to paste from selection", error);
    }
  };

  useEffect(() => {
    const unlisten = listen<string>("clipboard-changed", getClipboardHistory);

    return () => {
      unlisten.then((unlisten) => unlisten());
    };
  }, []);

  return (
    <div className="menu">
      <div className="menu__content">
        <div className="menu__label">
          <span className="menu__text">History</span>
        </div>

        <div className="menu__history">
          {clipboard.map((item, idx) => (
            <button
              className="menu__item"
              key={idx}
              onClick={() => pasteFromSelection(item.text)}
            >
              <span className="menu__title menu__text">
                {idx}. {item.text}
              </span>
            </button>
          ))}
        </div>
      </div>

      <div className="menu__footer">
        <div className="menu__separator" />

        <button className="menu__item" onClick={clearHistory}>
          <span className="menu__title menu__text">Clear History</span>
        </button>

        <button className="menu__item">
          <span className="menu__title menu__text">Preferences...</span>
        </button>

        <div className="menu__separator" />

        <button className="menu__item" onClick={quitClipbox}>
          <span className="menu__title menu__text">Quit Clipbox</span>
        </button>
      </div>
    </div>
  );
}

export default App;
