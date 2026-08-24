# GSV-167 native validation gap

Automated unit tests cover tray construction retention and failure, removal
lifecycle ordering through a fake manager adapter, menu action mapping, primary
Linux picker reachability, repeated activation, and trayless Settings and Quit
keyboard activation. They do not exercise a native tray host or desktop shell.

No native desktop session is available in this environment, so the following
manual checks remain outstanding:

- GNOME on X11: verify the tray icon is visible and legible, and that Open
  Picker, Settings, and Quit work from its menu.
- GNOME on Wayland: verify the same tray menu behavior when a compatible tray
  host is available, and verify normal launch and the trayless controls when it
  is absent.
- macOS: verify the existing template tray icon remains visually correct and
  that quitting removes it from the menu bar.
