# Fixing Handy on Ubuntu 26.04 GNOME Wayland

I tested this procedure on my own Wayland system and it resolved the issue for me. The steps below document the exact configuration and verification process I used.

The Handy shortcut itself is not changed. Users should continue using whatever transcription shortcut they already have configured in Handy.

I had an issue with Handy after upgrading to **Ubuntu 26.04.1 LTS**.

My system was using:

* Ubuntu 26.04.1 LTS
* GNOME
* Wayland
* Handy 0.9.8
* `Ctrl+Space` as my own Handy transcription shortcut

Handy was not working correctly after the upgrade.

The following is the exact configuration and sequence that fixed it for me.

> **Important:** `Ctrl+Space` is only the shortcut I had configured in Handy. You do **not** need to use `Ctrl+Space`. Keep whatever transcription shortcut you have configured in Handy.

---

## 1. Confirm you are using Wayland

Run:

```bash
echo "$XDG_SESSION_TYPE"
```

It should return:

```text
wayland
```

---

## 2. Check that your user is in the `input` group

Run:

```bash
id
```

On my system, the output included:

```text
groups=...,995(input)
```

If your user is not in the `input` group, add it with:

```bash
sudo usermod -aG input "$USER"
```

Then log out and back in before continuing.

---

## 3. Check `/dev/uinput`

Run:

```bash
ls -l /dev/uinput
```

On my working system, this eventually showed:

```text
crw-rw---- root input ... /dev/uinput
```

The important parts are:

```text
root:input
```

and permissions equivalent to:

```text
0660
```

---

## 4. Check the existing Ubuntu udev rule

Before creating any custom rule, check whether Ubuntu already provides a rule for `/dev/uinput`:

```bash
grep -R 'uinput' /etc/udev/rules.d /usr/lib/udev/rules.d 2>/dev/null
```

On my system, Ubuntu already had:

```text
/usr/lib/udev/rules.d/80-uinput.rules:KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"
```

**No custom udev rule was needed.**

---

## 5. Install `ydotool`

Install the Ubuntu package:

```bash
sudo apt install ydotool
```

Check that it is installed:

```bash
command -v ydotool
```

It should return:

```text
/usr/bin/ydotool
```

On Ubuntu 26.04, the `ydotool` package also provides `ydotoold` and its user systemd service.

---

## 6. Start the `ydotool` user service

Start the service:

```bash
systemctl --user start ydotool
```

Then check its status:

```bash
systemctl --user status ydotool --no-pager
```

It should show:

```text
Active: active (running)
```

We also verified the service definition with:

```bash
systemctl --user cat ydotool
```

The relevant part showed:

```text
[Service]
Type=simple
Restart=always
ExecStart=/usr/bin/ydotoold
```

---

## 7. Test `ydotool` independently

Before changing Handy, test whether `ydotool` itself can type.

Put the cursor into a text field or terminal and run:

```bash
ydotool type "HELLO FROM YDOTOOL"
```

It should type:

```text
HELLO FROM YDOTOOL
```

into the focused application.

This test worked successfully on my system.

At this point, the underlying `ydotool`/`uinput` input mechanism was working independently of Handy.

---

## 8. Configure Handy to use `ydotool`

Handy's settings file is:

```text
~/.local/share/com.pais.handy/settings_store.json
```

The existing configuration contained:

```json
"typing_tool": "auto"
```

We changed it to:

```json
"typing_tool": "ydotool"
```

The command we used was:

```bash
sed -i 's/"typing_tool": "auto"/"typing_tool": "ydotool"/' ~/.local/share/com.pais.handy/settings_store.json
```

---

## 9. Restart Handy

We restarted Handy with:

```bash
pkill handy
handy --start-hidden &
```

The Handy log confirmed that it loaded:

```text
typing_tool: Ydotool
```

At this point, `ydotool` was configured for text insertion, but Handy was still using its Tauri keyboard implementation.

---

## 10. Change Handy from Tauri to HandyKeys

The Handy settings contained:

```json
"keyboard_implementation": "tauri"
```

We changed it to:

```json
"keyboard_implementation": "handy_keys"
```

using:

```bash
sed -i 's/"keyboard_implementation": "tauri"/"keyboard_implementation": "handy_keys"/' ~/.local/share/com.pais.handy/settings_store.json
```

The relevant configuration was then:

```json
"keyboard_implementation": "handy_keys",
"typing_tool": "ydotool"
```

This was the important change for the shortcut handling.

---

## 11. Restart Handy again

After changing the keyboard implementation, we restarted Handy:

```bash
pkill handy
handy --start-hidden &
```

The startup log contained:

```text
[handy_app_lib::shortcut::handy_keys][INFO] handy-keys manager thread started
```

followed by:

```text
[handy_app_lib::shortcut::handy_keys][INFO] handy-keys shortcuts initialized
```

and:

```text
[handy_app_lib::commands][INFO] Shortcuts initialized successfully
```

These messages confirmed that the `handy_keys` shortcut manager initialized successfully.

---

## 12. Keep your existing Handy shortcut

The shortcut itself does not need to be changed.

My configured shortcut was:

```text
Ctrl+Space
```

That was simply my personal Handy configuration.

If your Handy installation uses another shortcut, continue using that shortcut.

The important changes were:

```text
typing_tool = ydotool
keyboard_implementation = handy_keys
```

---

## 13. Test Handy

Open a normal application with a text field.

Use your configured Handy transcription shortcut.

Speak a sentence.

For example:

```text
Hello, hello, Test One, Test Two.
```

On my system, Handy successfully transcribed the speech and inserted the resulting text into the focused application.

That was the final confirmation that the complete setup was working.

---

## 14. Verify that `ydotool` is still running

Run:

```bash
systemctl --user status ydotool --no-pager
```

It should show:

```text
Active: active (running)
```

---

## 15. Verify `/dev/uinput`

Run:

```bash
ls -l /dev/uinput
```

The working configuration showed:

```text
crw-rw---- root input ... /dev/uinput
```

---

## 16. Verify your `input` group membership

Run:

```bash
id
```

Confirm that:

```text
input
```

is listed.

---

## 17. Verify the Handy startup

After restarting Handy, the startup log should contain:

```text
handy-keys manager thread started
```

and:

```text
handy-keys shortcuts initialized
```

These are the relevant messages showing that the HandyKeys shortcut manager initialized.

---

# 18. Final working configuration

The configuration that fixed the problem on my system was:

```text
Ubuntu 26.04.1
GNOME
Wayland
     │
     ▼
   Handy
     │
     ├── handy_keys
     │
     └── ydotool
             │
             ▼
        /dev/uinput
             │
             ▼
     Active application
```

The two important Handy settings were:

```json
"typing_tool": "ydotool",
"keyboard_implementation": "handy_keys"
```

The system also had:

```text
User → input group
/dev/uinput → root:input, 0660
ydotoold → running as a user systemd service
```

My existing Handy shortcut remained unchanged.

---

# Quick version

For a system where the user is already in the `input` group and `/dev/uinput` already has the required permissions, the essential commands from the procedure were:

```bash
sudo apt install ydotool

systemctl --user start ydotool

ydotool type "HELLO FROM YDOTOOL"
```

If that works, configure Handy:

```bash
pkill handy

sed -i 's/"typing_tool": "auto"/"typing_tool": "ydotool"/' \
~/.local/share/com.pais.handy/settings_store.json

sed -i 's/"keyboard_implementation": "tauri"/"keyboard_implementation": "handy_keys"/' \
~/.local/share/com.pais.handy/settings_store.json

handy --start-hidden &
```

Then use your existing Handy transcription shortcut and test speech-to-text.

---

## Result

The combination that fixed Handy for me was:

```text
typing_tool = ydotool
keyboard_implementation = handy_keys
```

with `/dev/uinput` accessible to the `input` group.

The decisive configuration change was switching:

```text
keyboard_implementation = tauri
```

to:

```text
keyboard_implementation = handy_keys
```

After that change, Handy initialized the `handy_keys` shortcut manager successfully, and the end-to-end Handy speech-to-text test worked again.
