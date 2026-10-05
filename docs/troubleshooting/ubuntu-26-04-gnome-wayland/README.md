# Handy on Ubuntu 26.04 GNOME Wayland

Tested on Ubuntu 26.04.1 LTS, GNOME Wayland, Handy 0.9.8.

The fix is to use `ydotool` for typing and `handy_keys` for shortcuts.

*Note: Run commands one by one in terminal*

## 1. Check Wayland and uinput

```bash
echo "$XDG_SESSION_TYPE"
id
ls -l /dev/uinput
grep -R 'uinput' /etc/udev/rules.d /usr/lib/udev/rules.d 2>/dev/null
```

You should be using `Wayland`, be in the `input` group, and have `/dev/uinput` owned by `root:input` with mode `0660`. Ubuntu provides the required udev rule in `80-uinput.rules`.

If you are not in `input`:

```bash
sudo usermod -aG input "$USER"
```

Log out and back in after adding the group.

## 2. Install and test ydotool

```bash
sudo apt install ydotool
systemctl --user start ydotool
systemctl --user status ydotool --no-pager
ydotool type "HELLO FROM YDOTOOL"
```

The service should show `active (running)` and the test should type into the focused application.

## 3. Configure Handy

Edit Handy's settings with:

```bash
sed -i 's/"typing_tool": "auto"/"typing_tool": "ydotool"/' ~/.local/share/com.pais.handy/settings_store.json
sed -i 's/"keyboard_implementation": "tauri"/"keyboard_implementation": "handy_keys"/' ~/.local/share/com.pais.handy/settings_store.json
```

Restart Handy:

```bash
pkill handy
handy --start-hidden &
```

Check the Handy log for:

```text
handy-keys manager thread started
handy-keys shortcuts initialized
```

Then use your existing Handy shortcut and test dictation.

## 4. Hide the overlay

GNOME has no layer shell, so the recording overlay is a regular window. When it is visible, it can take the focus and the text is not pasted into your application.

In Handy settings, set **Overlay** to **None**.

## 5. Install wl-clipboard

On Wayland, Handy writes the clipboard with `wl-copy` when it is installed:

```bash
sudo apt install wl-clipboard
```

## 6. Non-QWERTY keyboard layouts

`ydotool` sends physical keys, not characters:

- **Clipboard (Ctrl+V)** and **Clipboard (Ctrl+Shift+V)** press the key at the QWERTY `V` position. On layouts where this key is not `v`, such as bépo or Dvorak, the application gets another shortcut and nothing is pasted.
- **Clipboard (Shift+Insert)** works on any layout, but fails while a modifier of your shortcut is still held (for example Ctrl with `Ctrl+Space`).

To paste with `Ctrl+V` on any layout, use an external script that presses the key that types `v` on your layout. Find the QWERTY key at the same position as `v` on your layout, and its code in `/usr/include/linux/input-event-codes.h`. For example, on bépo `v` is on the QWERTY `U` key (`KEY_U`, 22), and on Dvorak it is on the QWERTY `.` key (`KEY_DOT`, 52).

Create `~/.local/bin/handy-paste`, here for bépo:

```sh
#!/bin/sh
printf '%s' "$1" | wl-copy
sleep 0.1
ydotool key 29:1 22:1 22:0 29:0
```

Make it executable:

```bash
chmod +x ~/.local/bin/handy-paste
```

In Handy settings, set **Paste Method** to **External Script** and the script path to `/home/<user>/.local/bin/handy-paste`.
