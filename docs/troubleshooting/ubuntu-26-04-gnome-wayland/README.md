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
